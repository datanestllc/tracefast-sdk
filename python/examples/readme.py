"""Every example of the README, type-checked with mypy; not run."""

from __future__ import annotations

import asyncio
import os

from tracefast import AsyncTraceFast, TraceFast, TraceFastError


def readme(address: str, usdc: str) -> None:
    with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"], timeout=30.0, max_retries=2, user_agent="my-app/1.4") as tf:
        block = tf.blocks.latest()["data"]
        print(block["number"], block["hash"])

        caps = tf.contracts.capabilities("0xdac17f958d2ee523a2206206994597c13d831ec7")["data"]
        for c in caps["capabilities"]:
            print(c["power"], "inCode" in c, "availableNow" in c, "usedBefore" in c)

        found = tf.resolve(q="usdc")
        print(found["data"])

        res = tf.addresses.attributes_at_block(address, block=20_000_000)
        print(res["meta"].get("asOfBlock"), res["meta"]["docs"])

        for impl in tf.contracts.proxy(usdc)["data"]["implementations"]:
            if "supersededBy" not in impl:
                state = "not stated"
            elif impl["supersededBy"] is None:
                state = "current"
            else:
                state = f"replaced by {impl['supersededBy']}"
            print(state)

        try:
            tf.addresses.portfolio(address, block=30_000_000)
        except TraceFastError as e:
            if e.code == "block_not_available" and e.details is not None:
                print("covered up to", e.details["maxAvailableBlock"])
            else:
                raise

        for page in tf.addresses.transactions_pages(address, limit=100):
            for tx in page["data"]["transactions"]:
                print(tx["hash"])

        tf.addresses.transactions(address, direction="in", limit=100)
        tf.blocks.at_time(timestamp=1767225600)
        tf.tokens.price_history(usdc, from_=1767225600, to=1767312000)
        tf.pools.for_pair(token_a=usdc, token_b="0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")
        tf.risk.blacklist_status(address)
        tf.consensus.missed_slots()
        tf.transactions.storage_writes("0x" + "0" * 64)


async def readme_async(address: str) -> None:
    async with AsyncTraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
        latest = await tf.blocks.latest()
        print(latest["data"]["number"])
        async for page in tf.addresses.transactions_pages(address, limit=100):
            print(page["meta"]["docs"])


if __name__ == "__main__":
    asyncio.run(readme_async("0x"))
