"""The transport every method goes through. Not part of the public API.

One configuration check (SPEC §1), one error mapping (SPEC §4), one retry
policy (SPEC §5) and one pager (SPEC §6), over a sync `httpx.Client` or an
async `httpx.AsyncClient`.
"""

from __future__ import annotations

import asyncio
import email.utils
import ipaddress
import logging
import math
import random
import time
from collections.abc import AsyncGenerator, AsyncIterator, Awaitable, Callable, Generator, Iterator, Mapping
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, TypeAlias
from urllib.parse import quote, urlsplit

import httpx

from ._errors import TraceFastError, make_error
from ._version import VERSION

DEFAULT_BASE_URL = "https://tracefast.xyz/api/v1"
DEFAULT_TIMEOUT = 30.0
DEFAULT_MAX_RETRIES = 2
DEFAULT_MAX_RETRY_WAIT = 60.0
BACKOFF_START = 0.5
BACKOFF_CAP = 8.0

log = logging.getLogger("tracefast")

Query: TypeAlias = "Mapping[str, Any] | None"


# ---- requests


def segment(value: str | int) -> str:
    """Percent-encode one path segment, like JavaScript's `encodeURIComponent`.

    Raises:
        ValueError: The value is empty, `.` or `..` — URL normalisation would
            send the call to a different endpoint.
    """
    text = str(value)
    if text in ("", ".", ".."):
        raise ValueError("TraceFast: a path value may not be empty, '.' or '..'")
    return quote(text, safe="!~*'()")


def _wire(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def _params(query: Query) -> httpx.QueryParams:
    """The query string: a parameter left as `None` is not sent (SPEC §2)."""
    return httpx.QueryParams([(k, _wire(v)) for k, v in (query or {}).items() if v is not None])


# ---- configuration


def _check_key(api_key: object) -> str:
    # The messages never repeat the key. Ordinals are compared one by one: an
    # encode() would carry the key in its UnicodeEncodeError.
    if not isinstance(api_key, str) or not api_key:
        raise ValueError("TraceFast: api_key is required")
    if any(not 0x21 <= ord(c) <= 0x7E for c in api_key):
        raise ValueError(
            "TraceFast: api_key may hold only visible ASCII characters "
            "(no spaces, line breaks or typographic characters); check how it was copied"
        )
    return api_key


def _check_base_url(base_url: object) -> str:
    if not isinstance(base_url, str):
        raise TypeError("TraceFast: base_url must be a string")
    parts = urlsplit(base_url)
    host = (parts.hostname or "").lower()
    try:
        loopback = host == "localhost" or ipaddress.ip_address(host).is_loopback
    except ValueError:
        loopback = False
    if not host or not (parts.scheme == "https" or (parts.scheme == "http" and loopback)):
        raise ValueError("TraceFast: base_url must be an https URL (plain http only for a loopback host)")
    if parts.query or parts.fragment:
        raise ValueError("TraceFast: base_url may not carry a query or a fragment")
    return base_url.rstrip("/")


def _number(name: str, value: object, *, minimum: float, integer: bool = False, inclusive: bool = True) -> Any:
    kinds = (int,) if integer else (int, float)
    if isinstance(value, bool) or not isinstance(value, kinds):
        raise TypeError(f"TraceFast: {name} must be {'an integer' if integer else 'a number'}")
    if not math.isfinite(value) or value < minimum or (not inclusive and value == minimum):
        bound = f"{'at least' if inclusive else 'more than'} {minimum:g}"
        raise ValueError(f"TraceFast: {name} must be {bound}")
    return value


class Settings:
    """The checked configuration of a client. The key never shows in its repr."""

    __slots__ = ("_api_key", "base_url", "chain_id", "headers", "max_retries", "max_retry_wait", "timeout", "user_agent")

    def __init__(
        self,
        *,
        api_key: str,
        base_url: str = DEFAULT_BASE_URL,
        chain_id: int = 1,
        timeout: float = DEFAULT_TIMEOUT,
        max_retries: int = DEFAULT_MAX_RETRIES,
        max_retry_wait: float = DEFAULT_MAX_RETRY_WAIT,
        user_agent: str | None = None,
    ) -> None:
        self._api_key = _check_key(api_key)
        self.base_url = _check_base_url(base_url)
        self.chain_id: int = _number("chain_id", chain_id, minimum=1, integer=True)
        self.timeout = float(_number("timeout", timeout, minimum=0, inclusive=False))
        self.max_retries: int = _number("max_retries", max_retries, minimum=0, integer=True)
        self.max_retry_wait = float(_number("max_retry_wait", max_retry_wait, minimum=0))
        if user_agent is not None and (
            not isinstance(user_agent, str) or any(not 0x20 <= ord(c) <= 0x7E for c in user_agent)
        ):
            raise ValueError("TraceFast: user_agent may hold only printable ASCII characters")
        self.user_agent = user_agent or None
        self.headers = {
            "accept": "application/json",
            "user-agent": f"tracefast-python/{VERSION}" + (f" {user_agent}" if user_agent else ""),
            "x-api-key": self._api_key,
        }

    def replace(self, **changes: Any) -> Settings:
        """A copy with the given fields changed; `None` keeps a field as it is."""
        current: dict[str, Any] = {
            "api_key": self._api_key,
            "base_url": self.base_url,
            "chain_id": self.chain_id,
            "timeout": self.timeout,
            "max_retries": self.max_retries,
            "max_retry_wait": self.max_retry_wait,
            "user_agent": self.user_agent,
        }
        current.update({k: v for k, v in changes.items() if v is not None})
        return Settings(**current)

    def __repr__(self) -> str:
        return f"Settings(base_url={self.base_url!r}, chain_id={self.chain_id!r})"


# ---- answers


def backoff(attempt: int) -> float:
    """Exponential backoff from 0.5 s with jitter, each wait at most 8 s."""
    return float(min(BACKOFF_CAP, BACKOFF_START * 2**attempt * (0.5 + random.random() / 2)))


def parse_retry_after(header: str | None) -> float | None:
    """`Retry-After` in seconds (delta-seconds or an HTTP date), exactly as sent."""
    if not header:
        return None
    try:
        s = float(header)
    except ValueError:
        s = math.nan
    if math.isfinite(s):
        return s if s >= 0 else None
    try:
        at = email.utils.parsedate_to_datetime(header)
    except (TypeError, ValueError, IndexError):
        return None
    if at.tzinfo is None:
        at = at.replace(tzinfo=timezone.utc)
    return float(max(0, math.ceil((at - datetime.now(timezone.utc)).total_seconds())))


def _unexpected(res: httpx.Response, why: str, **fields: Any) -> TraceFastError:
    return make_error(
        status=res.status_code,
        code="unexpected_response",
        message=why,
        request_id=res.headers.get("x-request-id"),
        **fields,
    )


def to_error(res: httpx.Response) -> TraceFastError:
    """The error an answer that is not a success stands for."""
    retry_after = parse_retry_after(res.headers.get("retry-after"))
    header_id = res.headers.get("x-request-id")
    if res.is_redirect or 300 <= res.status_code < 400:
        return _unexpected(res, "a redirect, which the SDK never follows", retry_after=retry_after)
    try:
        body = res.json()
    except ValueError:
        body = None
    e = body.get("error") if isinstance(body, dict) else None
    if not (isinstance(e, dict) and isinstance(e.get("code"), str)):
        return _unexpected(res, "an answer without the API's error body", retry_after=retry_after)
    details = e.get("details") if isinstance(e.get("details"), dict) else None

    def text(value: Any) -> str | None:
        return value if isinstance(value, str) else None

    return make_error(
        status=res.status_code,
        code=e["code"],
        message=text(e.get("message")) or e["code"],
        reason=text(e.get("reason")) or text((details or {}).get("reason")),
        bucket=text(e.get("bucket")) or text((details or {}).get("bucket")),
        details=details,
        request_id=text(e.get("requestId")) or header_id,
        retry_after=retry_after,
        body=e,
    )


def envelope_of(res: httpx.Response) -> Any:
    """The `{data, meta}` of a success, or the error that says it is not one."""
    try:
        body = res.json()
    except ValueError:
        raise _unexpected(res, "a success whose body is not JSON") from None
    if not (isinstance(body, dict) and "data" in body and isinstance(body.get("meta"), dict)):
        raise _unexpected(res, "a success whose body is not the API's envelope")
    return body


@dataclass
class _Failure:
    """One attempt that did not succeed: the error, its cause, and whether to retry."""

    error: TraceFastError
    cause: BaseException | None
    retry: bool


def _detached(e: httpx.HTTPError) -> BaseException:
    """The same failure without the request it carries: its headers hold the key."""
    try:
        clone: BaseException = type(e)(str(e))
    except Exception:  # an exception type with another constructor
        clone = httpx.HTTPError(str(e))
    clone.__cause__ = e.__cause__
    clone.__suppress_context__ = True
    return clone.with_traceback(e.__traceback__)


def _transport_failure(e: httpx.HTTPError, status: int, timeout: float) -> _Failure:
    """Map an httpx failure; `status` is the answer's when its status line arrived.

    The cause attached is a copy of the httpx exception without its request.
    """
    failure = _map_transport_failure(e, status, timeout)
    if failure.cause is not None:
        failure.cause = _detached(e)
    return failure


def _map_transport_failure(e: httpx.HTTPError, status: int, timeout: float) -> _Failure:
    if isinstance(e, httpx.TimeoutException):
        return _Failure(make_error(status=status, code="timeout", message=f"no answer within {timeout:g} s"), e, False)
    if isinstance(e, (httpx.LocalProtocolError, httpx.UnsupportedProtocol)):
        # The request itself could not be sent. httpx's text may quote a header
        # value — the key — so neither its message nor the exception goes along.
        error = make_error(status=status, code="connection_error", message="the request could not be sent as built")
        return _Failure(error, None, False)
    if isinstance(e, httpx.DecodingError):
        error = make_error(status=status, code="unexpected_response", message="a body that does not match its Content-Encoding")
        return _Failure(error, e, False)
    if isinstance(e, httpx.TransportError):
        message = f"the connection failed or broke ({type(e).__name__})"
        return _Failure(make_error(status=status, code="connection_error", message=message), e, True)
    return _Failure(make_error(status=status, code="connection_error", message=type(e).__name__), e, False)


class _Base:
    """What both transports share."""

    settings: Settings

    def __repr__(self) -> str:
        return f"{type(self).__name__}(base_url={self.base_url!r}, chain_id={self.chain_id!r})"

    @property
    def base_url(self) -> str:
        return self.settings.base_url

    @property
    def chain_id(self) -> int:
        return self.settings.chain_id

    def _wait(self, failure: _Failure, attempt: int, path: str) -> float | None:
        """How long to wait before the next attempt, or None to give up now."""
        s, e = self.settings, failure.error
        if attempt >= s.max_retries:
            return None
        wait: float | None = None
        if e.code == "connection_error":
            wait = backoff(attempt) if failure.retry else None
        elif e.code == "quota_exceeded":
            wait = None
        elif e.status == 429:
            if e.retry_after is not None and e.retry_after <= s.max_retry_wait:
                wait = e.retry_after
            elif e.retry_after is not None:
                log.debug("not retrying %s: Retry-After %g s is over max_retry_wait", path, e.retry_after)
        elif e.status == 503:
            wait = backoff(attempt) if e.retry_after is None else min(e.retry_after, s.max_retry_wait)
        if wait is not None:
            log.debug(
                "retrying %s in %.2f s after %s (retry %d of %d, request %s)",
                path,
                wait,
                e.status or e.code,
                attempt + 1,
                s.max_retries,
                e.request_id,
            )
        return wait

    def _closed(self) -> RuntimeError:
        return RuntimeError("TraceFast: the client is closed; create a new one")


def _check_limit(name: str, value: int | None) -> None:
    if value is not None:
        _number(name, value, minimum=0, integer=True)


def _next_cursor(page: Any, previous: str | None) -> str | None:
    """`data.nextCursor` while there are more pages; None on the last one."""
    data = page.get("data") if isinstance(page, dict) else None
    nxt = data.get("nextCursor") if isinstance(data, dict) else None
    if nxt is None:
        return None
    if not isinstance(nxt, str) or not nxt:
        raise make_error(status=200, code="unexpected_response", message="an empty nextCursor")
    if nxt == previous:
        raise make_error(status=200, code="unexpected_response", message="a nextCursor equal to the one sent; the walk would not end")
    return nxt


def _rows(page: Any, field: str) -> list[Any]:
    data = page.get("data") if isinstance(page, dict) else None
    rows = data.get(field) if isinstance(data, dict) else None
    if not isinstance(rows, list):
        raise make_error(status=200, code="unexpected_response", message=f"a page without the data.{field} array")
    return rows


class SyncHttp(_Base):
    """The sync transport, over `httpx.Client`."""

    def __init__(self, settings: Settings, client: httpx.Client, sleep: Callable[[float], None] = time.sleep) -> None:
        self.settings = settings
        self.client = client
        self.sleep = sleep

    def _once(self, url: str, params: httpx.QueryParams) -> Any | _Failure:
        s = self.settings
        try:
            request = self.client.build_request("GET", url, params=params, headers=s.headers, timeout=s.timeout)
            res = self.client.send(request, stream=True, follow_redirects=False)
        except httpx.HTTPError as e:
            return _transport_failure(e, 0, s.timeout)
        try:
            if res.is_redirect or 300 <= res.status_code < 400:
                return _Failure(to_error(res), None, False)
            try:
                res.read()
            except httpx.HTTPError as e:
                return _transport_failure(e, res.status_code, s.timeout)
            if res.is_success:
                return envelope_of(res)
            return _Failure(to_error(res), None, False)
        finally:
            res.close()

    def get(self, path: str, query: Query) -> Any:
        if self.client.is_closed:
            raise self._closed()
        url = self.settings.base_url + path
        params = _params(query)
        attempt = 0
        while True:
            outcome = self._once(url, params)
            if not isinstance(outcome, _Failure):
                return outcome
            wait = self._wait(outcome, attempt, path)
            if wait is None:
                raise outcome.error from outcome.cause
            self.sleep(wait)
            attempt += 1

    def pages(self, path: str, query: Query, max_pages: int | None = None) -> Iterator[Any]:
        _check_limit("max_pages", max_pages)
        return self._pages(path, dict(query or {}), max_pages)

    def _pages(self, path: str, params: dict[str, Any], max_pages: int | None) -> Generator[Any, None, None]:
        cursor = params.get("cursor")
        count = 0
        while max_pages is None or count < max_pages:
            page = self.get(path, params)
            count += 1
            yield page
            cursor = _next_cursor(page, cursor)
            if cursor is None:
                return
            params = {**params, "cursor": cursor}

    def items(self, path: str, query: Query, field: str, max_items: int | None = None) -> Iterator[Any]:
        _check_limit("max_items", max_items)
        return self._items(path, dict(query or {}), field, max_items)

    def _items(self, path: str, params: dict[str, Any], field: str, max_items: int | None) -> Iterator[Any]:
        if max_items == 0:
            return
        count = 0
        pages = self._pages(path, params, None)
        try:
            for page in pages:
                for row in _rows(page, field):
                    yield row
                    count += 1
                    if max_items is not None and count >= max_items:
                        return
        finally:
            pages.close()


class AsyncHttp(_Base):
    """The async transport, over `httpx.AsyncClient`."""

    def __init__(
        self,
        settings: Settings,
        client: httpx.AsyncClient,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> None:
        self.settings = settings
        self.client = client
        self.sleep = sleep

    async def _once(self, url: str, params: httpx.QueryParams) -> Any | _Failure:
        s = self.settings
        try:
            request = self.client.build_request("GET", url, params=params, headers=s.headers, timeout=s.timeout)
            res = await self.client.send(request, stream=True, follow_redirects=False)
        except httpx.HTTPError as e:
            return _transport_failure(e, 0, s.timeout)
        try:
            if res.is_redirect or 300 <= res.status_code < 400:
                return _Failure(to_error(res), None, False)
            try:
                await res.aread()
            except httpx.HTTPError as e:
                return _transport_failure(e, res.status_code, s.timeout)
            if res.is_success:
                return envelope_of(res)
            return _Failure(to_error(res), None, False)
        finally:
            await res.aclose()

    async def get(self, path: str, query: Query) -> Any:
        if self.client.is_closed:
            raise self._closed()
        url = self.settings.base_url + path
        params = _params(query)
        attempt = 0
        while True:
            outcome = await self._once(url, params)
            if not isinstance(outcome, _Failure):
                return outcome
            wait = self._wait(outcome, attempt, path)
            if wait is None:
                raise outcome.error from outcome.cause
            await self.sleep(wait)
            attempt += 1

    def pages(self, path: str, query: Query, max_pages: int | None = None) -> AsyncIterator[Any]:
        _check_limit("max_pages", max_pages)
        return self._pages(path, dict(query or {}), max_pages)

    async def _pages(self, path: str, params: dict[str, Any], max_pages: int | None) -> AsyncGenerator[Any, None]:
        cursor = params.get("cursor")
        count = 0
        while max_pages is None or count < max_pages:
            page = await self.get(path, params)
            count += 1
            yield page
            cursor = _next_cursor(page, cursor)
            if cursor is None:
                return
            params = {**params, "cursor": cursor}

    def items(self, path: str, query: Query, field: str, max_items: int | None = None) -> AsyncIterator[Any]:
        _check_limit("max_items", max_items)
        return self._items(path, dict(query or {}), field, max_items)

    async def _items(self, path: str, params: dict[str, Any], field: str, max_items: int | None) -> AsyncIterator[Any]:
        if max_items == 0:
            return
        count = 0
        pages = self._pages(path, params, None)
        try:
            async for page in pages:
                for row in _rows(page, field):
                    yield row
                    count += 1
                    if max_items is not None and count >= max_items:
                        return
        finally:
            await pages.aclose()
