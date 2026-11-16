"""Static checks of the public types, read by `mypy --strict`; never run."""

from __future__ import annotations

from collections.abc import AsyncIterator, Iterator

from typing_extensions import assert_type

from tracefast import (
    AsyncTraceFast,
    PermissionDeniedError,
    RateLimitError,
    TraceFast,
    TraceFastError,
)
from tracefast.types import (
    BlockResponse,
    BlockTransactionsResponse,
    ProxyImplementationsResponse,
    Transaction,
    TransactionResponse,
)


def sync_client(tf: TraceFast) -> None:
    assert_type(tf.blocks.latest(), BlockResponse)
    assert_type(tf.blocks.get(25_920_000), BlockResponse)
    assert_type(tf.transactions.get("0x" + "0" * 64), TransactionResponse)
    proxy = tf.contracts.proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
    assert_type(proxy, ProxyImplementationsResponse)
    for impl in proxy["data"]["implementations"]:
        if "supersededBy" in impl:
            assert_type(impl["supersededBy"], "str | None")
    assert_type(tf.blocks.transactions_pages(1, max_pages=2), Iterator[BlockTransactionsResponse])
    assert_type(tf.blocks.transactions_items(1, max_items=10), Iterator[Transaction])
    assert_type(tf.with_options(timeout=5.0), TraceFast)
    assert_type(tf.blocks.latest()["meta"].get("asOfBlock"), "int | None")


async def async_client(tf: AsyncTraceFast) -> None:
    assert_type(await tf.blocks.latest(), BlockResponse)
    assert_type(tf.blocks.transactions_pages(1), AsyncIterator[BlockTransactionsResponse])
    assert_type(tf.blocks.transactions_items(1), AsyncIterator[Transaction])
    assert_type(tf.with_options(max_retries=0), AsyncTraceFast)


def errors(e: TraceFastError) -> None:
    assert_type(e.status, int)
    assert_type(e.code, str)
    assert_type(e.reason, "str | None")
    assert_type(e.bucket, "str | None")
    assert_type(e.retry_after, "float | None")
    assert_type(e.request_id, "str | None")
    if isinstance(e, PermissionDeniedError | RateLimitError):
        assert_type(e, "PermissionDeniedError | RateLimitError")


def wrong_calls(tf: TraceFast) -> None:
    tf.blocks.get("1")  # type: ignore[arg-type]
    tf.addresses.transactions("0x", unknown=1)  # type: ignore[call-arg]
