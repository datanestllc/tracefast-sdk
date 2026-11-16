"""The transport every method goes through. Not part of the public API.

One retry policy (SPEC §5) and one error mapping (SPEC §4), over a sync
``httpx.Client`` or an async ``httpx.AsyncClient``.
"""

from __future__ import annotations

import asyncio
import email.utils
import math
import random
import time
from collections.abc import AsyncIterator, Awaitable, Callable, Iterator, Mapping
from datetime import datetime, timezone
from typing import Any, TypeAlias
from urllib.parse import quote

import httpx

from ._errors import TraceFastError
from ._version import VERSION

DEFAULT_BASE_URL = "https://tracefast.xyz/api/v1"
MAX_RETRY_AFTER_S = 60.0

Query: TypeAlias = "Mapping[str, Any] | None"


def segment(value: str | int) -> str:
    """Percent-encode one path segment, like JavaScript's ``encodeURIComponent``."""
    return quote(str(value), safe="!~*'()")


def _wire(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def _params(query: Query) -> httpx.QueryParams:
    """The query string: a parameter left as ``None`` is not sent (SPEC §2)."""
    return httpx.QueryParams([(k, _wire(v)) for k, v in (query or {}).items() if v is not None])


def backoff(attempt: int) -> float:
    """Exponential backoff from 0.5 s with jitter, in seconds."""
    return float(0.5 * 2**attempt * (0.5 + random.random() / 2))


def parse_retry_after(header: str | None) -> float | None:
    """``Retry-After`` in seconds (delta-seconds or an HTTP date), capped at 60."""
    if not header:
        return None
    try:
        s = float(header)
    except ValueError:
        s = math.nan
    if math.isfinite(s):
        return min(s, MAX_RETRY_AFTER_S) if s >= 0 else None
    try:
        at = email.utils.parsedate_to_datetime(header)
    except (TypeError, ValueError):
        return None
    if at.tzinfo is None:
        at = at.replace(tzinfo=timezone.utc)
    wait = math.ceil((at - datetime.now(timezone.utc)).total_seconds())
    return float(min(max(0, wait), MAX_RETRY_AFTER_S))


def retry_delay(status: int, retry_after: float | None, attempt: int) -> float | None:
    """How long to wait before retrying this answer, or None if it is not retried."""
    if status == 429:
        return retry_after  # without Retry-After: a quota or a cap — not retried
    if status == 503:
        return backoff(attempt) if retry_after is None else retry_after
    return None


def to_error(res: httpx.Response) -> TraceFastError:
    retry_after = parse_retry_after(res.headers.get("retry-after"))
    header_id = res.headers.get("x-request-id")
    try:
        body = res.json()
    except ValueError:
        body = None
    e = body.get("error") if isinstance(body, dict) else None
    if isinstance(e, dict) and isinstance(e.get("code"), str):
        details = e.get("details")
        request_id = e.get("requestId")
        message = e.get("message")
        return TraceFastError(
            status=res.status_code,
            code=e["code"],
            message=message if isinstance(message, str) else e["code"],
            details=details if isinstance(details, dict) else None,
            request_id=request_id if isinstance(request_id, str) else header_id,
            retry_after=retry_after,
        )
    return TraceFastError(
        status=res.status_code,
        code="unexpected_response",
        message=f"HTTP {res.status_code} without the API's error body",
        request_id=header_id,
        retry_after=retry_after,
    )


def body_of(res: httpx.Response) -> Any:
    try:
        return res.json()
    except ValueError:
        raise TraceFastError(
            status=res.status_code,
            code="unexpected_response",
            message=f"HTTP {res.status_code} with a body that is not JSON",
            request_id=res.headers.get("x-request-id"),
        ) from None


def next_cursor(page: Any) -> str | None:
    """``data.nextCursor`` while there are more pages; None on the last one."""
    data = page.get("data") if isinstance(page, dict) else None
    nxt = data.get("nextCursor") if isinstance(data, dict) else None
    return nxt if isinstance(nxt, str) and nxt else None


class _Config:
    """What both transports share. The key never shows in its repr."""

    def __init__(
        self,
        *,
        api_key: str,
        base_url: str,
        chain_id: int,
        timeout: float,
        max_retries: int,
        user_agent: str | None,
    ) -> None:
        if not isinstance(api_key, str) or not api_key:
            raise ValueError("TraceFast: api_key is required")
        if max_retries < 0:
            raise ValueError("TraceFast: max_retries must be 0 or more")
        self.chain_id = chain_id
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.max_retries = max_retries
        self.user_agent = user_agent
        self._headers = {
            "accept": "application/json",
            "x-api-key": api_key,
            "user-agent": f"tracefast-python/{VERSION}" + (f" {user_agent}" if user_agent else ""),
        }

    def __repr__(self) -> str:
        return f"{type(self).__name__}(base_url={self.base_url!r}, chain_id={self.chain_id!r})"

    def _connection_error(self, e: Exception) -> TraceFastError:
        return TraceFastError(status=0, code="connection_error", message=f"could not reach {self.base_url}")

    def _timeout_error(self) -> TraceFastError:
        return TraceFastError(status=0, code="timeout", message=f"no answer within {self.timeout:g} s")


class SyncHttp(_Config):
    """The sync transport, over ``httpx.Client``."""

    def __init__(self, *, client: httpx.Client, sleep: Callable[[float], None] = time.sleep, **config: Any) -> None:
        super().__init__(**config)
        self.client = client
        self.sleep = sleep

    def get(self, path: str, query: Query) -> Any:
        url = self.base_url + path
        params = _params(query)
        attempt = 0
        while True:
            try:
                res = self.client.get(url, params=params, headers=self._headers, timeout=self.timeout)
            except httpx.TimeoutException as e:
                raise self._timeout_error() from e
            except httpx.TransportError as e:
                if attempt < self.max_retries:
                    self.sleep(backoff(attempt))
                    attempt += 1
                    continue
                raise self._connection_error(e) from e
            if res.is_success:
                return body_of(res)
            error = to_error(res)
            wait = retry_delay(res.status_code, error.retry_after, attempt)
            if wait is not None and attempt < self.max_retries:
                self.sleep(wait)
                attempt += 1
                continue
            raise error

    def pages(self, path: str, query: Query) -> Iterator[Any]:
        params = dict(query or {})
        while True:
            page = self.get(path, params)
            yield page
            cursor = next_cursor(page)
            if cursor is None:
                return
            params = {**params, "cursor": cursor}


class AsyncHttp(_Config):
    """The async transport, over ``httpx.AsyncClient``."""

    def __init__(
        self,
        *,
        client: httpx.AsyncClient,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
        **config: Any,
    ) -> None:
        super().__init__(**config)
        self.client = client
        self.sleep = sleep

    async def get(self, path: str, query: Query) -> Any:
        url = self.base_url + path
        params = _params(query)
        attempt = 0
        while True:
            try:
                res = await self.client.get(url, params=params, headers=self._headers, timeout=self.timeout)
            except httpx.TimeoutException as e:
                raise self._timeout_error() from e
            except httpx.TransportError as e:
                if attempt < self.max_retries:
                    await self.sleep(backoff(attempt))
                    attempt += 1
                    continue
                raise self._connection_error(e) from e
            if res.is_success:
                return body_of(res)
            error = to_error(res)
            wait = retry_delay(res.status_code, error.retry_after, attempt)
            if wait is not None and attempt < self.max_retries:
                await self.sleep(wait)
                attempt += 1
                continue
            raise error

    async def pages(self, path: str, query: Query) -> AsyncIterator[Any]:
        params = dict(query or {})
        while True:
            page = await self.get(path, params)
            yield page
            cursor = next_cursor(page)
            if cursor is None:
                return
            params = {**params, "cursor": cursor}
