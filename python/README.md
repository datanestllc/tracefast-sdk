# tracefast

The official Python SDK for the [TraceFast API](https://docs.tracefast.xyz/api):
Ethereum blocks, transactions, execution traces, tokens, contracts, DEX pools,
the consensus layer, and what TraceFast proves about an address — now and at
any past block.

- Every one of the API's 97 methods, typed with `TypedDict`s from the API catalogue
- A synchronous client and an `asyncio` client with the same methods
- The whole answer — `data` and `meta` (`asOfBlock`, `coverage`)
- One exception type with stable codes, retries that follow `Retry-After`
- Generators over paged methods
- Two dependencies: `httpx` and `typing-extensions`

> **Private preview.** Not on PyPI yet; the API may still change before 1.0.

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Methods](#methods)
- [Responses](#responses)
- [Errors](#errors)
- [Retries and timeouts](#retries-and-timeouts)
- [Pagination](#pagination)
- [Async](#async)
- [Development](#development)

## Install

While the package is private, install it from this repository (Python 3.10 or
later):

```bash
pip install "git+https://github.com/datanestllc/tracefast-sdk.git#subdirectory=python"
# or
uv add "tracefast @ git+https://github.com/datanestllc/tracefast-sdk.git#subdirectory=python"
```

Once published it will be `pip install tracefast`.

## Quick start

```python
import os
from tracefast import TraceFast

with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
    # The newest block.
    block = tf.blocks.latest()["data"]
    print(block["number"], block["hash"])

    # What a contract can do — and what has already been done with it.
    caps = tf.contracts.capabilities("0xdac17f958d2ee523a2206206994597c13d831ec7")["data"]
    for c in caps["capabilities"]:
        print(c["power"], "inCode" in c, "availableNow" in c, "usedBefore" in c)

    # Resolve a name to the addresses it can mean.
    found = tf.resolve(q="usdc")
```

Get a key at [tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
Every method needs a plan — **Enriched Data**, **Intelligence** or
**Investigation**; each method's docstring names it.

> The API key is a server-side secret: keep it in the environment or a secret
> store, never in code you ship to users.

## Configuration

```python
tf = TraceFast(
    api_key=os.environ["TRACEFAST_API_KEY"],  # required
    timeout=30.0,           # seconds, for each phase of a call (connect, read)
    max_retries=2,          # see "Retries"; 0 turns them off
    user_agent="my-app/1.4",
    # base_url="https://tracefast.xyz/api/v1", chain_id=1, http_client=httpx.Client(...)
)
```

| Argument | Default | |
|---|---|---|
| `api_key` | — | your `tf_live_…` key; sent in the `x-api-key` header only |
| `base_url` | `https://tracefast.xyz/api/v1` | |
| `chain_id` | `1` | Ethereum mainnet, the only chain served today |
| `timeout` | `30.0` | seconds, per phase of a call |
| `max_retries` | `2` | |
| `user_agent` | — | appended to `tracefast-python/<version>` |
| `http_client` | a new `httpx.Client` | bring your own for proxies or custom transports; the SDK closes only a client it created |

Use the client as a context manager, or call `tf.close()` when you are done.

## Methods

Methods are grouped as in the API documentation. Path parameters are
positional, query parameters are keyword-only, and a parameter you leave as
`None` is not sent.

| Namespace | Methods | For example |
|---|---|---|
| `tf.addresses` | 22 | `portfolio(address, block=...)`, `transactions(address, direction="in", limit=100)`, `attributes(address)` |
| `tf.blocks` | 10 | `latest()`, `get(number)`, `at_time(timestamp=...)`, `transactions(number, limit=100)` |
| `tf.transactions` | 10 | `get(hash)`, `events(hash)`, `failures(hash)`, `storage_writes(hash)` |
| `tf.traces` | 2 | `get(hash)`, `frames(hash, limit=100)` |
| `tf.contracts` | 19 | `get(address)`, `proxy(address)`, `capabilities(address)`, `decompilation(address)` |
| `tf.tokens` | 8 | `get(token)`, `holders(token)`, `price(token, block=...)`, `price_history(token, from_=..., to=...)` |
| `tf.pools` | 9 | `get(pool)`, `for_pair(token_a=..., token_b=...)`, `swaps(pool)`, `reserves(pool)` |
| `tf.consensus` | 11 | `current()`, `queues()`, `validator(index)`, `missed_slots()` |
| `tf.risk` | 5 | `blacklist_status(address)`, `blacklist_events(address)` |
| `tf.resolve` | 1 | `resolve(q="usdc")` |

Names follow Python: `fromBlock` is `from_block`, `tokenA` is `token_a`, and
`from` — a Python keyword — is `from_`. On the wire they keep the API's names.

Every method's docstring carries the method's description, the plan it needs,
whether it is **Beta**, and a link to its page on
[docs.tracefast.xyz](https://docs.tracefast.xyz/api/methods).

## Responses

A method returns the API's whole answer as a plain `dict`, typed with a
`TypedDict` from `tracefast.types`:

```python
res = tf.addresses.attributes_at_block(address, block=20_000_000)
res["meta"]["asOfBlock"]   # the block the answer is true at
res["meta"]["docs"]        # the documentation page of the method
```

The types say exactly what the API can send:

- `NotRequired[T]` — the key may be **absent**;
- `T | None` — the value may be **`None`** (JSON `null`);
- both — and they mean different things. For a proxy implementation,
  `supersededBy` is an address when it was replaced, `None` when it is still
  current, and absent when storage cannot say:

```python
for impl in tf.contracts.proxy(usdc)["data"]["implementations"]:
    if "supersededBy" not in impl:
        state = "not stated"
    elif impl["supersededBy"] is None:
        state = "current"
    else:
        state = f"replaced by {impl['supersededBy']}"
```

An absent fact is **not proven** — it never means "no". Amounts, balances and
wei are decimal strings; block numbers and counts are `int`. Enumerations are
open (`Literal[...] | str`): the API may add a value.

## Errors

Every failure is a `TraceFastError`:

```python
from tracefast import TraceFastError

try:
    tf.addresses.portfolio(address, block=30_000_000)
except TraceFastError as e:
    if e.code == "block_not_available" and e.details:
        print("covered up to", e.details["maxAvailableBlock"])
    else:
        raise
```

| Attribute | |
|---|---|
| `status` | HTTP status; `0` when no answer arrived |
| `code` | the stable word to branch on: `bad_request`, `not_found`, `block_not_available`, `rate_limited`, `quota_exceeded`, … — or the SDK's own `unexpected_response`, `connection_error`, `timeout` |
| `reason` | `details["reason"]`, the stable sub-code, when given |
| `details` | what to act on, e.g. `maxAvailableBlock`, `maxLimit` |
| `request_id` | quote it when you contact support |
| `retry_after` | seconds the API asked to wait, from `Retry-After` |

The message is for humans and may be reworded — never parse it. The full list
of codes is on [conventions](https://docs.tracefast.xyz/api/conventions#errors).

## Retries and timeouts

Only answers that say "try again" are retried, up to `max_retries` times:

- `429` **with** `Retry-After` — after the time it states (at most 60 s);
- `503` — after `Retry-After`, or with exponential backoff and jitter;
- a connection failure before any answer.

A `429` without `Retry-After` (a daily quota, a concurrency cap) and every other
error are raised at once. A redirect is never followed, so the key never travels
to another host. Every method is a read, so a retry never repeats a side effect.

## Pagination

Paged methods return one page and a `nextCursor` while there are more. The
`*_pages` companion walks them all:

```python
for page in tf.addresses.transactions_pages(address, limit=100):
    for tx in page["data"]["transactions"]:
        print(tx["hash"])
```

It stops when `nextCursor` is absent — a page may hold fewer rows than `limit`
and still not be the last. Each page is a whole response, `meta` included.

## Async

`AsyncTraceFast` has the same namespaces and methods, as coroutines:

```python
import asyncio
from tracefast import AsyncTraceFast

async def main() -> None:
    async with AsyncTraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
        latest = await tf.blocks.latest()
        async for page in tf.addresses.transactions_pages(address, limit=100):
            ...

asyncio.run(main())
```

## Development

The code in `src/tracefast/_generated/` is generated from the API catalogue — do
not edit it; see the [repository README](../README.md). The tests read the
generator's model, so Node.js must be on the `PATH`.

```bash
uv venv && uv pip install -e '.[dev]'
uv run mypy --strict src
uv run pytest -q     # every method against its recorded live answer, plus behaviour tests
uv build
```

## License

[MIT](../LICENSE)
