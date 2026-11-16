"""The two clients: ``TraceFast`` (sync) and ``AsyncTraceFast`` (async)."""

from __future__ import annotations

from types import TracebackType

import httpx

from ._generated.api import AsyncClientBase, ClientBase
from ._http import DEFAULT_BASE_URL, AsyncHttp, SyncHttp


class TraceFast(ClientBase):
    """The TraceFast API client.

    Example::

        from tracefast import TraceFast

        with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
            res = tf.contracts.proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
            print(res["data"], res["meta"])

    Args:
        api_key: Your TraceFast API key (``tf_live_…``). Server-side only. It is
            sent in the ``x-api-key`` header and nowhere else.
        base_url: Default ``https://tracefast.xyz/api/v1``.
        chain_id: Default ``1``, Ethereum mainnet — the only chain served today.
        timeout: Seconds to wait for each phase of a call (connect, read).
            Default 30.
        max_retries: Retries of answers that say "try again" (``429`` with
            ``Retry-After``, ``503``, a connection failure). Default 2.
        user_agent: Appended to the ``User-Agent`` header, e.g. ``my-app/1.2``.
        http_client: An ``httpx.Client`` to send the requests with (proxies,
            transports). The client does not close one it was given.
    """

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        chain_id: int = 1,
        timeout: float = 30.0,
        max_retries: int = 2,
        user_agent: str | None = None,
        http_client: httpx.Client | None = None,
    ) -> None:
        self._owns_client = http_client is None
        http = SyncHttp(
            client=http_client if http_client is not None else httpx.Client(),
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            user_agent=user_agent,
        )
        super().__init__(http)

    def close(self) -> None:
        """Close the connections, unless the ``httpx.Client`` was given."""
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
    """The TraceFast API client for ``asyncio``.

    The same namespaces and methods as ``TraceFast``; every method is a
    coroutine and every ``…_pages`` method an async iterator.

    Example::

        from tracefast import AsyncTraceFast

        async with AsyncTraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
            res = await tf.blocks.latest()
            async for page in tf.addresses.transactions_pages(address, limit=100):
                ...

    Args:
        api_key: Your TraceFast API key (``tf_live_…``). Server-side only. It is
            sent in the ``x-api-key`` header and nowhere else.
        base_url: Default ``https://tracefast.xyz/api/v1``.
        chain_id: Default ``1``, Ethereum mainnet — the only chain served today.
        timeout: Seconds to wait for each phase of a call (connect, read).
            Default 30.
        max_retries: Retries of answers that say "try again" (``429`` with
            ``Retry-After``, ``503``, a connection failure). Default 2.
        user_agent: Appended to the ``User-Agent`` header, e.g. ``my-app/1.2``.
        http_client: An ``httpx.AsyncClient`` to send the requests with. The
            client does not close one it was given.
    """

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        chain_id: int = 1,
        timeout: float = 30.0,
        max_retries: int = 2,
        user_agent: str | None = None,
        http_client: httpx.AsyncClient | None = None,
    ) -> None:
        self._owns_client = http_client is None
        http = AsyncHttp(
            client=http_client if http_client is not None else httpx.AsyncClient(),
            api_key=api_key,
            base_url=base_url,
            chain_id=chain_id,
            timeout=timeout,
            max_retries=max_retries,
            user_agent=user_agent,
        )
        super().__init__(http)

    async def close(self) -> None:
        """Close the connections, unless the ``httpx.AsyncClient`` was given."""
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
