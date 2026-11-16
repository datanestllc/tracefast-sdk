"""The two clients: `TraceFast` (sync) and `AsyncTraceFast` (async)."""

from __future__ import annotations

from types import TracebackType

import httpx

from ._generated.api import AsyncClientBase, ClientBase
from ._http import (
    DEFAULT_BASE_URL,
    DEFAULT_MAX_RETRIES,
    DEFAULT_MAX_RETRY_WAIT,
    DEFAULT_TIMEOUT,
    AsyncHttp,
    Settings,
    SyncHttp,
)


class TraceFast(ClientBase):
    """The TraceFast API client.

    Example:
        ```python
        import os
        from tracefast import TraceFast

        with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
            res = tf.contracts.proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
            print(res["data"], res["meta"])
        ```

    Args:
        api_key: Your TraceFast API key (`tf_live_…`). Server-side only. It is
            sent in the `x-api-key` header and nowhere else. It must be made of
            visible ASCII characters; anything else raises `ValueError`.
        base_url: Default `https://tracefast.xyz/api/v1`. Must be `https`
            (plain `http` only for a loopback host).
        chain_id: Default `1`, Ethereum mainnet — the only chain served today.
        timeout: Seconds allowed for each phase of an attempt — connecting,
            sending, waiting for and reading the answer. Default 30.
        max_retries: Retries of answers that say "try again" (`429` with a short
            `Retry-After`, `503`, a connection failure). Default 2; `0` turns
            retries off.
        max_retry_wait: The longest `Retry-After` the client waits out, in
            seconds. A `429` asking for longer is raised at once. Default 60.
        user_agent: Appended to the `User-Agent` header, e.g. `my-app/1.2`.
        http_client: An httpx client to send the requests with (proxies,
            transports). Redirects are still never followed. The SDK does not
            close a client it was given.

    Raises:
        ValueError: The key, `base_url` or a number is not valid. The message
            never repeats the key.
    """

    _owns_client: bool

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        chain_id: int = 1,
        timeout: float = DEFAULT_TIMEOUT,
        max_retries: int = DEFAULT_MAX_RETRIES,
        max_retry_wait: float = DEFAULT_MAX_RETRY_WAIT,
        user_agent: str | None = None,
        http_client: httpx.Client | None = None,
    ) -> None:
        # Check everything before a connection pool exists.
        settings = Settings(
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            max_retry_wait=max_retry_wait,
            user_agent=user_agent,
        )
        if http_client is not None and not isinstance(http_client, httpx.Client):
            raise TypeError("TraceFast: http_client must be an httpx.Client")
        self._owns_client = http_client is None
        super().__init__(SyncHttp(settings, http_client if http_client is not None else httpx.Client()))

    def with_options(
        self,
        *,
        api_key: str | None = None,
        base_url: str | None = None,
        chain_id: int | None = None,
        timeout: float | None = None,
        max_retries: int | None = None,
        max_retry_wait: float | None = None,
        user_agent: str | None = None,
    ) -> TraceFast:
        """A copy of this client with some options changed.

        The copy shares this client's connections: it is cheap to make for one
        call or one part of a program. Closing the copy does nothing; closing
        this client closes the connections of both. An option left as None
        keeps its value.

        Returns:
            The new client.

        Raises:
            ValueError: A changed option is not valid.
        """
        settings = self._http.settings.replace(
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            max_retry_wait=max_retry_wait,
            user_agent=user_agent,
        )
        copy = object.__new__(type(self))
        ClientBase.__init__(copy, SyncHttp(settings, self._http.client, self._http.sleep))
        copy._owns_client = False
        return copy

    def close(self) -> None:
        """Close the connections, unless the `httpx.Client` was given or this is a copy."""
        if self._owns_client:
            self._http.client.close()

    def __enter__(self) -> TraceFast:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        self.close()

    def __repr__(self) -> str:
        return f"TraceFast(base_url={self._http.base_url!r}, chain_id={self._http.chain_id!r})"


class AsyncTraceFast(AsyncClientBase):
    """The TraceFast API client for `asyncio`.

    The same namespaces and methods as `TraceFast`; every method is a coroutine
    and every `…_pages` and `…_items` method an async iterator. Cancelling the
    task cancels the call: `asyncio.CancelledError` is never caught.

    Example:
        ```python
        import os
        from tracefast import AsyncTraceFast

        async with AsyncTraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
            res = await tf.blocks.latest()
        ```

    Args:
        api_key: Your TraceFast API key (`tf_live_…`). Server-side only. It is
            sent in the `x-api-key` header and nowhere else. It must be made of
            visible ASCII characters; anything else raises `ValueError`.
        base_url: Default `https://tracefast.xyz/api/v1`. Must be `https`
            (plain `http` only for a loopback host).
        chain_id: Default `1`, Ethereum mainnet — the only chain served today.
        timeout: Seconds allowed for each phase of an attempt — connecting,
            sending, waiting for and reading the answer. Default 30.
        max_retries: Retries of answers that say "try again" (`429` with a short
            `Retry-After`, `503`, a connection failure). Default 2; `0` turns
            retries off.
        max_retry_wait: The longest `Retry-After` the client waits out, in
            seconds. A `429` asking for longer is raised at once. Default 60.
        user_agent: Appended to the `User-Agent` header, e.g. `my-app/1.2`.
        http_client: An httpx client to send the requests with (proxies,
            transports). Redirects are still never followed. The SDK does not
            close a client it was given.

    Raises:
        ValueError: The key, `base_url` or a number is not valid. The message
            never repeats the key.
    """

    _owns_client: bool

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        chain_id: int = 1,
        timeout: float = DEFAULT_TIMEOUT,
        max_retries: int = DEFAULT_MAX_RETRIES,
        max_retry_wait: float = DEFAULT_MAX_RETRY_WAIT,
        user_agent: str | None = None,
        http_client: httpx.AsyncClient | None = None,
    ) -> None:
        settings = Settings(
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            max_retry_wait=max_retry_wait,
            user_agent=user_agent,
        )
        if http_client is not None and not isinstance(http_client, httpx.AsyncClient):
            raise TypeError("AsyncTraceFast: http_client must be an httpx.AsyncClient")
        self._owns_client = http_client is None
        super().__init__(AsyncHttp(settings, http_client if http_client is not None else httpx.AsyncClient()))

    def with_options(
        self,
        *,
        api_key: str | None = None,
        base_url: str | None = None,
        chain_id: int | None = None,
        timeout: float | None = None,
        max_retries: int | None = None,
        max_retry_wait: float | None = None,
        user_agent: str | None = None,
    ) -> AsyncTraceFast:
        """A copy of this client with some options changed.

        The copy shares this client's connections: it is cheap to make for one
        call or one part of a program. Closing the copy does nothing; closing
        this client closes the connections of both. An option left as None
        keeps its value.

        Returns:
            The new client.

        Raises:
            ValueError: A changed option is not valid.
        """
        settings = self._http.settings.replace(
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            max_retry_wait=max_retry_wait,
            user_agent=user_agent,
        )
        copy = object.__new__(type(self))
        AsyncClientBase.__init__(copy, AsyncHttp(settings, self._http.client, self._http.sleep))
        copy._owns_client = False
        return copy

    async def close(self) -> None:
        """Close the connections, unless the `httpx.AsyncClient` was given or this is a copy."""
        if self._owns_client:
            await self._http.client.aclose()

    async def __aenter__(self) -> AsyncTraceFast:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        await self.close()

    def __repr__(self) -> str:
        return f"AsyncTraceFast(base_url={self._http.base_url!r}, chain_id={self._http.chain_id!r})"
