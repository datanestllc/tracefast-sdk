# tracefast

**[TraceFast](https://tracefast.xyz) shows what happened on Ethereum, down to
execution: who took part, what moved, and which code ran.** It reads every block
from its own archive node and states a fact only when it is proven — always with
the block it holds for. Behind one API: blocks, transactions and full call
traces; tokens, DEX pools and the consensus layer; and the TraceFast knowledge
graph — what an address is now and what it was at past blocks, how its contract
is built, what its owner can do and has already done, and how it links to other
addresses — with ecosystem labels that name the protocol and role of each new
pool, market or vault from the block its factory created it. Built for
compliance teams, exchanges, auditors and investigators — and open to explore in
the app at [tracefast.xyz](https://tracefast.xyz).

`tracefast` is the official Python SDK for the
[TraceFast API](https://docs.tracefast.xyz/api): every method, typed, with a
synchronous and an `asyncio` client.

> **Private preview.** The package is not on PyPI yet. Only people with access
> to the private repository can install it, and its interface may change before
> 1.0.

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [Quick start](#quick-start)
- [Authentication and plans](#authentication-and-plans)
- [Configuration](#configuration)
- [Methods](#methods)
- [Responses](#responses)
- [Errors](#errors)
- [Retries and timeouts](#retries-and-timeouts)
- [Pagination](#pagination)
- [Async](#async)
- [Types](#types)
- [Versioning and stability](#versioning-and-stability)
- [Support](#support)
- [Development](#development)
- [License](#license)

## Requirements

- Python 3.10 or later.
- A TraceFast API key on a paid plan. See
  [tracefast.xyz/pricing](https://tracefast.xyz/pricing) for the plans.

## Install

Install from the private repository:

```bash
pip install "git+https://github.com/datanestllc/tracefast-sdk.git#subdirectory=python"
# or
uv add "tracefast @ git+https://github.com/datanestllc/tracefast-sdk.git#subdirectory=python"
```

When the package is published, the command will be `pip install tracefast`.

## Quick start

```python
import os

from tracefast import TraceFast

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
```

Run it: save it as `quickstart.py`, then
`TRACEFAST_API_KEY=tf_live_… python quickstart.py`.

The three methods above need the **Enriched Data** plan.

## Authentication and plans

The client sends your key in the `x-api-key` header, and nowhere else. The key
is a secret for server-side code. Do not use it in code that runs in a browser
or on a user's device.

Each method needs a plan: **Enriched Data**, **Intelligence** or
**Investigation**. The docstring of each method names its plan, and your editor
shows it on hover. When your plan does not include a method, the call raises
`PermissionDeniedError` with the reason `plan_scope_missing`. Its `details` name
the plan the method needs and the plan you have:

```python
from tracefast import PermissionDeniedError, TraceFast


def plan_refusal(tf: TraceFast) -> None:
    try:
        tf.addresses.transactions("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", limit=10)
    except PermissionDeniedError as e:
        if e.reason != "plan_scope_missing":
            raise
        details = e.details or {}
        print("needs", details.get("minimumPlan"), "- you have", details.get("currentPlan"))
```

A missing or invalid key raises `AuthenticationError` (`401`). The key is
checked when you create the client: it must be made of visible ASCII
characters. A key with a space, a line break or a typographic character raises
`ValueError`, and the message does not repeat the key.

## Configuration

```python
import os

import httpx

from tracefast import TraceFast

tf = TraceFast(
    api_key=os.environ["TRACEFAST_API_KEY"],
    timeout=30.0,
    max_retries=2,
    max_retry_wait=60.0,
    user_agent="my-app/1.4",
)
```

| Option | Default | Meaning |
|---|---|---|
| `api_key` | required | Your `tf_live_…` key. Sent in the `x-api-key` header only. |
| `base_url` | `https://tracefast.xyz/api/v1` | Must be `https`. Plain `http` is accepted only for a loopback host (tests). |
| `chain_id` | `1` | Ethereum mainnet, the only chain served today. |
| `timeout` | `30.0` | Seconds for each phase of one attempt, as httpx counts them: getting a connection, connecting, sending, and each wait for data. It does not limit the whole call. |
| `max_retries` | `2` | Retries of answers that say "try again". `0` turns retries off. |
| `max_retry_wait` | `60.0` | The longest `Retry-After` the client waits for, in seconds. A `429` that asks for longer is raised at once. |
| `user_agent` | `None` | Added to `tracefast-python/<version>` in the `User-Agent` header. |
| `http_client` | `None` | Your own `httpx.Client` (or `httpx.AsyncClient` for `AsyncTraceFast`), for proxies or transports. |

If you give your own `http_client`:

- The SDK still never follows a redirect, even when your client is set to
  follow them. A `3xx` answer raises `UnexpectedResponseError`, so the key is
  never sent to another host.
- The SDK does not close your client. Close it yourself.

```python
import os

import httpx

from tracefast import TraceFast


def own_http_client() -> None:
    with httpx.Client(proxy="http://proxy.example:3128") as http:
        tf = TraceFast(api_key=os.environ["TRACEFAST_API_KEY"], http_client=http)
        print(tf.blocks.latest()["data"]["number"])
```

`with_options(...)` returns a copy of the client with some options changed. The
copy shares the connections of the original. Closing the copy does nothing;
closing the original closes the connections of both.

```python
from tracefast import TraceFast


def slow_part(tf: TraceFast) -> None:
    patient = tf.with_options(timeout=120.0, max_retries=4)
    print(patient.traces.get("0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5")["meta"]["docs"])
```

Close the client when you are done with it: use `with TraceFast(...) as tf:`
or call `tf.close()`. A call on a closed client raises `RuntimeError`.

## Methods

The methods are in groups, in the same way as in the
[API documentation](https://docs.tracefast.xyz/api/methods). The client has
every method of the API.

| Group | For example |
|---|---|
| `tf.blocks` | `latest()`, `get(25_920_000)`, `at_time(timestamp=1767225600)`, `transactions(25_920_000, limit=100)` |
| `tf.transactions` | `get(hash)`, `events(hash)`, `failures(hash)`, `storage_writes(hash)` |
| `tf.traces` | `get(hash)`, `frames(hash, limit=100)` |
| `tf.contracts` | `get(address)`, `proxy(address)`, `capabilities(address)`, `selectors(address)` |
| `tf.addresses` | `get(address)`, `portfolio(address, block=20_000_000)`, `transactions(address, direction="sent", limit=50)`, `attributes(address)` |
| `tf.tokens` | `get(token)`, `holders(token, limit=10)`, `transfers(token, from_=address)`, `price_history(token, from_=1767225600, to=1767312000)` |
| `tf.pools` | `get(pool)`, `for_pair(token_a=usdc, token_b=weth)`, `swaps(pool)`, `reserves(pool)` |
| `tf.consensus` | `current()`, `queues()`, `validator(1)`, `missed_slots(limit=10)` |
| `tf.network` | `daily()`, `gas_daily()`, `top_pools(by="volume", days=7)`, `top_dex_tokens()` |
| `tf.lending` | `config("aave_v3")` |
| `tf.signals` | `recent(kind="account_drain")`, `kinds()`, `rates()` |
| `tf.risk` | `blacklist_status(address)`, `blacklist_events(address)` |
| `tf` itself | `tf.resolve(q="usdc")`: the addresses a name or a fragment can mean |

- Path parameters are positional, in the order of the path. Block numbers,
  slots and validator indexes are `int`.
- Query parameters are keyword arguments. A keyword argument that you do not
  set (or set to `None`) is not sent, and the API uses its default.
- Names are snake_case: `fromBlock` is `from_block`. A name that is a Python
  keyword gets a trailing underscore: the `from` parameter is `from_`.
- Addresses and hashes are sent as you give them. The API accepts any case and
  answers in lowercase.
- A path value that is empty, `.` or `..` raises `ValueError` before any
  request.

The docstring of each method gives its description, the plan it needs, whether
it is **Beta**, and the link to its documentation page.

## Responses

A method returns the whole answer of the API, `{"data": ..., "meta": ...}`, as
a plain `dict` typed with a `TypedDict`:

```python
from tracefast import TraceFast


def responses(tf: TraceFast) -> None:
    res = tf.addresses.get("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
    print(res["data"]["address"])
    print(res["meta"].get("asOfBlock"))  # the block the answer is true at
    print(res["meta"]["docs"])  # the documentation page of the method
    print(res["meta"]["cached"])  # True when the answer came from the cache
```

For most methods, `meta.asOfBlock` is the block the answer is true at. For the
at-block methods (`attributes_at_block`, `relations_at_block`), it is the
horizon of the history, and the block you asked about is `data["block"]`. Each
method's documentation page says what `asOfBlock` means for it.

**Absent, `None` and a value are three different answers.** A field that the
API can leave out is `NotRequired` and stays absent from the dict. A field that
can be `null` is typed `... | None`. When a field can be both, they mean
different things. For a proxy implementation, `supersededBy` is:

- absent: storage cannot say;
- `None`: no replacement was seen, the implementation is current;
- an address: the implementation was replaced by it.

Read such fields with `"key" in d` or `d.get("key")`, never `d["key"]`.

An absent fact is **not proven**. It never means "no".

Units are those of the API:

- Amounts in wei and gwei, and raw token amounts, are decimal strings
  (`valueWei`, `amountGwei`, `reserveRaw`). Use `int(...)` to calculate with
  them.
- Token amounts scaled by decimals, and USD values, are `float`. Use them for
  display only.
- Block numbers and counts are `int`.
- `meta["timestamp"]` is Unix time in **milliseconds**. Timestamps in `data`
  are Unix time in **seconds**, or RFC 3339 strings where the field says so.
  Timestamps you send (`timestamp`, `from_`, `to`) are Unix seconds.

Enumerations are open: the API can add a value. For example,
`tracefast.types.TokenClass` is `Literal["fungible", "nft"] | str`. Always
handle a value that you do not know.

## Errors

Every failure raises a `TraceFastError`:

```python
from tracefast import NotFoundError, TraceFast, TraceFastError, UnprocessableEntityError


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
```

| Attribute | Meaning |
|---|---|
| `status` | The HTTP status. `0` when no answer arrived. |
| `code` | The stable word to branch on: `bad_request`, `invalid_cursor`, `not_found`, `block_not_available`, `coverage_gap`, `auth_failed`, `forbidden`, `rate_limited`, `quota_exceeded`, … or one of the SDK's own: `unexpected_response`, `connection_error`, `timeout`. |
| `reason` | The stable sub-code, when there is one (`api_key_required`, `plan_scope_missing`, …). |
| `bucket` | On a `429`, the limit that refused the call (`base`, `concurrency`, …). |
| `details` | A dict of what to act on, when the API gives it (`maxAvailableBlock`, `maxLimit`, `minimumPlan`, `currentPlan`, …). Read it with `.get`. |
| `request_id` | The id of the request. Give it to support. |
| `retry_after` | Seconds from the `Retry-After` header, exactly as sent. |
| `body` | The raw `error` object of the answer, so a field the SDK does not know is kept. |
| `message` | `"<status> <code>: <message>"`, for people. It can change; never parse it. |

The subclass tells the kind of failure:

| Subclass | When |
|---|---|
| `BadRequestError` | `400` |
| `AuthenticationError` | `401` |
| `PermissionDeniedError` | `403` |
| `NotFoundError` | `404` |
| `UnprocessableEntityError` | `422` |
| `RateLimitError` | `429` |
| `InternalServerError` | `5xx` |
| `APIConnectionError` | `connection_error`: the connection failed or broke |
| `APITimeoutError` | `timeout` |
| `UnexpectedResponseError` | `unexpected_response`: a redirect, a proxy page, or a body that is not the API's |

`reason` comes from the error object when the gateway refuses the key, the
plan or a limit, and from `details["reason"]` otherwise. Branch on the
subclass, `code` and `reason`, never on `message`. The
[full list of codes](https://docs.tracefast.xyz/api/conventions#errors) is in
the API documentation.

## Retries and timeouts

The client retries only answers that say "try again", at most `max_retries`
times:

- `429` with a `Retry-After` of at most `max_retry_wait` seconds: the client
  waits that long, then retries.
- `503`: the client waits for `Retry-After` (at most `max_retry_wait`), or, if
  there is no `Retry-After`, for an exponential backoff from 0.5 s with jitter,
  at most 8 s for each wait.
- A connection failure, also one while the body is read: backoff as for `503`.

These are raised at once: a `429` without `Retry-After` or with a longer one
(`retry_after` tells the real wait), every `quota_exceeded`, a timeout, and all
other errors. Every method is a read, so a retry never repeats a side effect.

`timeout` applies to each phase of each attempt, not to the whole call. One
call makes at most `max_retries + 1` attempts, plus the waits between them
(each at most `max_retry_wait`). If you need a hard limit on a whole call, use
`asyncio.wait_for` with `AsyncTraceFast`.

Set the `tracefast` logger to `DEBUG` to see each retry decision (status, wait,
attempt and request id). The log never contains the key or the headers.

## Pagination

A paged method returns one page. `data["nextCursor"]` is present while there are
more pages, and absent on the last one. Each paged method has two walkers:

- `<method>_pages(...)` gives each whole response, `meta` included;
  `max_pages` sets a limit.
- `<method>_items(...)` gives the rows one by one; `max_items` sets a limit.

```python
from itertools import islice

from tracefast import TraceFast


def pagination(tf: TraceFast) -> None:
    for tx in tf.blocks.transactions_items(25_920_000, limit=100, max_items=250):
        print(tx["hash"], tx["valueWei"])

    for page in tf.tokens.transfers_pages("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", limit=100, max_pages=3):
        print(len(page["data"]["transfers"]), page["meta"].get("asOfBlock"))

    first = list(islice(tf.blocks.withdrawals_items(25_920_000), 5))
    print(first)
```

Always set a limit. The history of an active address or token has millions of
rows, and a walk without a limit reads all of them.

- A walker stops when `nextCursor` is absent, never on a short page: a page
  can have fewer rows than `limit` and still have a next page.
- A walker sends the cursor back unchanged, with the same other parameters. If
  you give `cursor=`, the walk starts from it.
- An empty `nextCursor`, or one equal to the previous cursor, raises
  `UnexpectedResponseError`.
- `400 invalid_cursor` raises `BadRequestError`. Discard the rows you collected
  and start again from the first page. For the lists of one block, it means the
  block was reorganised, and the rows you read belong to the old block.

The walkers use the plan of their method. The feeds of an address
(`addresses.transactions`, `addresses.token_transfers`,
`addresses.internal_transfers`) need the **Investigation** plan.

## Async

`AsyncTraceFast` has the same groups and methods. Each method is a coroutine,
and each walker is an async iterator:

```python
import asyncio
import os

from tracefast import AsyncTraceFast


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
```

Cancellation works as usual: when the task is cancelled, the call stops and
`asyncio.CancelledError` propagates. The SDK never catches it.

## Types

All response types are in `tracefast.types`, one `TypedDict` for each object
of the API catalogue. They work with mypy, Pyright and your editor:

```python
from tracefast import CATALOGUE_VERSION
from tracefast.types import ProxyImplementation, TokenClass


def describe(impl: ProxyImplementation, kind: TokenClass) -> str:
    return f"{impl['implementation']} ({kind}), catalogue {CATALOGUE_VERSION}"
```

At runtime a response is a plain `dict`. An unknown field in a response is kept
and does not cause an error. `CATALOGUE_VERSION` is the version of the API
catalogue that the package was generated from.

## Versioning and stability

- Versions `0.x` can change the interface of the package between minor
  versions.
- From `1.0`, the package follows semantic versioning.
- Methods marked **Beta** in their docstring can still change shape, and are
  not covered by these rules. See
  [stability](https://docs.tracefast.xyz/api/conventions#stability).

## Support

Report problems at
[github.com/datanestllc/tracefast-sdk/issues](https://github.com/datanestllc/tracefast-sdk/issues).
For a failed call, give the `request_id` of the error.

## Development

The code in `src/tracefast/_generated/` is generated from the API catalogue. Do
not edit it. To change it, edit `codegen/emit-python.mjs` and run the generator
from the repository root (see the
[repository README](https://github.com/datanestllc/tracefast-sdk#readme)):

```bash
node codegen/generate.mjs          # from the repository root; needs Node.js
cd python
uv sync --group dev
uv run pytest -q                   # every method against its recorded answer, and the behaviour tests
uv run mypy --strict               # src, examples and tests/static
uv build
```

The tests need Node.js, because they read the generator's model of the API.

## License

[MIT](https://github.com/datanestllc/tracefast-sdk/blob/main/LICENSE)
