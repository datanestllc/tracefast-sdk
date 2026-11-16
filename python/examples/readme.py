"""Every example of the README, checked by `mypy --strict`; not run (they need a key)."""

from __future__ import annotations

import asyncio
import os
from itertools import islice

import httpx

from tracefast import (
    CATALOGUE_VERSION,
    AsyncTraceFast,
    NotFoundError,
    PermissionDeniedError,
    TraceFast,
    TraceFastError,
    UnprocessableEntityError,
)
from tracefast.types import ProxyImplementation, TokenClass


def quick_start() -> None:
    with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
        # The newest block.
        block = tf.blocks.latest()["data"]
        print(block["number"], block["hash"], block["txCount"])

        # Every implementation the USDC proxy has delegated to, newest first.
        proxy = tf.contracts.proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")["data"]
        for impl in proxy["implementations"]:
            if "supersededBy" not in impl:
                state = "storage cannot say"
            elif impl["supersededBy"] is None:
                state = "current"
            else:
                state = f"replaced by {impl['supersededBy']}"
            print(impl["implementation"], impl["firstSeenBlock"], state)

        # One transaction by hash.
        tx = tf.transactions.get("0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5")["data"]
        print(tx["from"], tx.get("to"), tx["valueWei"], tx.get("status"))


def plan_refusal(tf: TraceFast) -> None:
    try:
        tf.addresses.transactions("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", limit=10)
    except PermissionDeniedError as e:
        if e.reason != "plan_scope_missing":
            raise
        details = e.details or {}
        print("needs", details.get("minimumPlan"), "- you have", details.get("currentPlan"))


def configuration() -> None:
    tf = TraceFast(
        api_key=os.environ["TRACEFAST_API_KEY"],
        timeout=30.0,
        max_retries=2,
        max_retry_wait=60.0,
        user_agent="my-app/1.4",
    )


def own_http_client() -> None:
    with httpx.Client(proxy="http://proxy.example:3128") as http:
        tf = TraceFast(api_key=os.environ["TRACEFAST_API_KEY"], http_client=http)
        print(tf.blocks.latest()["data"]["number"])


def slow_part(tf: TraceFast) -> None:
    patient = tf.with_options(timeout=120.0, max_retries=4)
    print(patient.traces.get("0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5")["meta"]["docs"])


def responses(tf: TraceFast) -> None:
    res = tf.addresses.get("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
    print(res["data"]["address"])
    print(res["meta"].get("asOfBlock"))  # the block the answer is true at
    print(res["meta"]["docs"])  # the documentation page of the method
    print(res["meta"]["cached"])  # True when the answer came from the cache


def errors(tf: TraceFast) -> None:
    try:
        tf.blocks.transactions(30_000_000, limit=10)
    except NotFoundError:
        print("no such block yet")
    except UnprocessableEntityError as e:
        if e.code != "block_not_available":
            raise
        print("served up to block", (e.details or {}).get("maxAvailableBlock"))
    except TraceFastError as e:
        print(e.status, e.code, e.reason, e.request_id)
        raise


def pagination(tf: TraceFast) -> None:
    for tx in tf.blocks.transactions_items(25_920_000, limit=100, max_items=250):
        print(tx["hash"], tx["valueWei"])

    for page in tf.tokens.transfers_pages("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", limit=100, max_pages=3):
        print(len(page["data"]["transfers"]), page["meta"].get("asOfBlock"))

    first = list(islice(tf.blocks.withdrawals_items(25_920_000), 5))
    print(first)


async def main() -> None:
    async with AsyncTraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
        block, proxy = await asyncio.gather(
            tf.blocks.latest(),
            tf.contracts.proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        )
        print(block["data"]["number"], len(proxy["data"]["implementations"]))

        async for tx in tf.blocks.transactions_items(25_920_000, max_items=20):
            print(tx["hash"])

        latest = await asyncio.wait_for(tf.blocks.latest(), timeout=10)
        print(latest["meta"]["timestamp"])


def describe(impl: ProxyImplementation, kind: TokenClass) -> str:
    return f"{impl['implementation']} ({kind}), catalogue {CATALOGUE_VERSION}"


if __name__ == "__main__":
    quick_start()
