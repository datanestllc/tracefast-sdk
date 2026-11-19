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

`tracefast` is the official Rust SDK for the
[TraceFast API](https://docs.tracefast.xyz/api): every method, typed, async on
Tokio.

> **Before 1.0**, a minor version may change the crate's API — see
> [Versioning and stability](#versioning-and-stability).

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
- [Types](#types)
- [Versioning and stability](#versioning-and-stability)
- [Support](#support)
- [Development](#development)
- [License](#license)

## Requirements

- Rust 1.88 or later.
- A Tokio runtime.
- A TraceFast API key on a paid plan — see <https://tracefast.xyz/pricing>.

## Install

```toml
[dependencies]
tracefast = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

TLS uses rustls with bundled root certificates by default. Choose another
backend with the crate features:

| Feature | |
|---|---|
| `rustls-tls` (default) | rustls, Mozilla root certificates bundled in the binary |
| `rustls-tls-native-roots` | rustls, the operating system's root certificates |
| `native-tls` | the platform's TLS library (OpenSSL, Secure Transport, SChannel) |
| `gzip`, `brotli` | accept compressed answers |
| `stream` | `futures_core::Stream` for the page and row walkers |

For example, `tracefast = { version = "0.1", default-features = false, features = ["native-tls"] }`.

## Quick start

```rust,no_run
use tracefast::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(std::env::var("TRACEFAST_API_KEY")?)?;

    // The newest block.
    let latest = client.blocks().latest().await?;
    println!("block {} {}", latest.data.number, latest.data.hash);

    // The implementations behind the USDC proxy. `superseded_by` has three states.
    let proxy = client.contracts().proxy("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").await?;
    for imp in &proxy.data.implementations {
        let state = match &imp.superseded_by {
            None => "not stated".to_string(),       // absent: storage cannot say
            Some(None) => "current".to_string(),    // null: no replacement observed
            Some(Some(next)) => format!("replaced by {next}"),
        };
        println!("{} {state}", imp.implementation);
    }

    // One transaction.
    let tx = client
        .transactions()
        .get("0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5", &tracefast::TransactionsGetParams::new())
        .await?;
    println!("{} sent {} wei in block {}", tx.data.from, tx.data.value_wei, tx.data.block_number);
    Ok(())
}
```

Run it: `TRACEFAST_API_KEY=tf_live_… cargo run`.

## Authentication and plans

The key travels only in the `x-api-key` header — never in a URL, an error or a
`Debug` output — and the client never follows a redirect, so the key never
reaches another host. The key is for server-side code. This SDK does not
support running in a browser (WebAssembly included) or in any program you ship
to other people's machines.

Get a key at <https://tracefast.xyz/settings/api/keys>. Every method needs a
plan — **Enriched Data**, **Intelligence** or **Investigation** — and each
method's documentation names it. A key whose plan does not include the method
gets a `403`: `ErrorKind::PermissionDenied` with the reason
`plan_scope_missing`, and `details` naming `minimumPlan` and `currentPlan`.
The plans are described at <https://docs.tracefast.xyz/api/authentication#plans>.

## Configuration

```rust,no_run
use std::time::Duration;

use tracefast::Client;

fn client(api_key: String) -> Result<Client, tracefast::ConfigError> {
    Client::builder()
        .api_key(api_key)                         // required
        .timeout(Duration::from_secs(30))         // per attempt, the body included
        .max_retries(2)                           // 0 turns retries off
        .max_retry_wait(Duration::from_secs(60))  // the longest Retry-After waited out
        .user_agent("my-app/1.4")                 // appended to the User-Agent header
        .build()
}
```

| Setting | Default | |
|---|---|---|
| `api_key` | — | your `tf_live_…` key: non-empty, visible ASCII only |
| `base_url` | `https://tracefast.xyz/api/v1` | `https` only (`http` for a loopback host), no query, no fragment |
| `chain_id` | `1` | Ethereum mainnet, the only chain served today |
| `timeout` | 30 s | one attempt, from connecting to the last byte of the body |
| `max_retries` | `2` | see [Retries and timeouts](#retries-and-timeouts) |
| `max_retry_wait` | 60 s | a longer `Retry-After` is returned to you, not waited out |
| `user_agent` | — | appended to `tracefast-rust/<version>` |

`build()` returns a `ConfigError` when a setting is wrong: no key, a key with a
space, a line break or a typographic character (the message never repeats the
key), a base URL that is not `https`, a user agent a header cannot carry.
`ConfigError` converts into `tracefast::Error`, so `?` works in a function that
returns `tracefast::Result`.

`Client` is cheap to clone. A copy with other settings shares the connection
pool:

```rust,no_run
use std::time::Duration;

async fn patient(client: &tracefast::Client) -> tracefast::Result<()> {
    let patient = client.with_timeout(Duration::from_secs(120)).with_max_retries(4);
    let block = patient.blocks().get(20_000_000).await?;
    println!("{}", block.data.hash);
    Ok(())
}
```

## Methods

Methods are grouped as in the API documentation. Path parameters are
positional (integers are `u64`); query parameters go in a params struct whose
setters you chain, and a parameter you do not set is not sent.

| Namespace | For example |
|---|---|
| `client.addresses()` | `get(address)`, `portfolio(address, &AddressesPortfolioParams::new().block(20_000_000))`, `transactions(address, &AddressesTransactionsParams::new().direction("sent").limit(100))`, `attributes(address)` |
| `client.blocks()` | `latest()`, `get(25_920_000)`, `at_time(&BlocksAtTimeParams::new(1_767_225_600))`, `transactions(25_920_000, &BlocksTransactionsParams::new().limit(100))` |
| `client.transactions()` | `get(hash, &TransactionsGetParams::new())`, `events(hash, &TransactionsEventsParams::new())`, `failures(hash)`, `storage_writes(hash, &TransactionsStorageWritesParams::new())` |
| `client.traces()` | `get(hash, &TracesGetParams::new())`, `frames(hash, &TracesFramesParams::new().limit(100))` |
| `client.contracts()` | `get(address)`, `proxy(address)`, `capabilities(address)`, `selectors(address, &ContractsSelectorsParams::new())` |
| `client.tokens()` | `get(token)`, `holders(token, &TokensHoldersParams::new().limit(10))`, `price(token, &TokensPriceParams::new().block(20_000_000))` |
| `client.pools()` | `get(pool)`, `for_pair(&PoolsForPairParams::new(token_a, token_b))`, `swaps(pool, &PoolsSwapsParams::new())`, `reserves(pool)` |
| `client.consensus()` | `current()`, `queues(&ConsensusQueuesParams::new())`, `validator(1)`, `missed_slots(&ConsensusMissedSlotsParams::new())` |
| `client.network()` | `daily(&NetworkDailyParams::new())`, `gas_daily(&NetworkGasDailyParams::new())`, `top_pools(&NetworkTopPoolsParams::new().by("volume").days(7))` |
| `client.lending()` | `config("aave_v3", &LendingConfigParams::new())` |
| `client.signals()` | `recent(&SignalsRecentParams::new().kind("account_drain"))`, `kinds()`, `rates()` |
| `client.risk()` | `blacklist_status(address)`, `blacklist_events(address, &RiskBlacklistEventsParams::new())` |
| `client.resolve(…)` | `resolve(&ResolveParams::new("usdc"))` |

Names follow Rust: `fromBlock` is `from_block`, `tokenA` is `token_a`. On the
wire they keep the API's names. Each method's documentation carries the
method's description, the plan it needs, **Beta** when it is in beta, and a link
to its page on <https://docs.tracefast.xyz/api/methods>.

A method returns a future that borrows only the client — not the namespace
handle, not the arguments — so futures can be built in a loop and awaited
together:

```rust,no_run
async fn proxies(client: &tracefast::Client, addresses: &[String]) -> tracefast::Result<()> {
    let calls: Vec<_> = addresses.iter().map(|a| client.contracts().proxy(a)).collect();
    for res in futures::future::join_all(calls).await {
        println!("{} implementations", res?.data.implementations.len());
    }
    Ok(())
}
```

(`join_all` is from the `futures` crate.) A path value that is empty, `.` or
`..` is refused before any request with an `ErrorKind::InvalidArgument` error
(code `invalid_argument`): URL normalisation would otherwise send the call to
another endpoint. Beyond that the SDK does not validate values; the API's
refusal says what is wrong.

## Responses

A method returns the API's whole answer, `data` and `meta`, never `data` alone:

```rust,no_run
async fn meta(client: &tracefast::Client) -> tracefast::Result<()> {
    let res = client.transactions().get("0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5", &tracefast::TransactionsGetParams::new()).await?;
    println!("as of block {:?}", res.meta.as_of_block);
    println!("cached: {}, documentation: {}", res.meta.cached, res.meta.docs);
    Ok(())
}
```

`meta.as_of_block` is, for most methods, the block the answer is true at. For
the at-block methods (`attributes_at_block`, `relations_at_block`) it is the
horizon of the history, and the point in time is `data.block`. Each method's
documentation says which.

The types say exactly what the API can send:

- `Option<T>` — the field may be absent, or (for a field that is always sent)
  may be `null`;
- `Option<Option<T>>` — the field may be absent **or** `null`, and the two mean
  different things: `None` is absent, `Some(None)` is `null`. For a proxy
  implementation, `superseded_by` is `None` when storage cannot say,
  `Some(None)` when the pointer is current, and `Some(Some(address))` when it
  was replaced (see the [quick start](#quick-start)).

An absent fact is **not proven** — it never means "no". Wei, gwei and raw token
amounts are decimal `String`s (the unit is in the field name); decimal-scaled
amounts and USD values are `f64`, for display; block numbers and counts are
`i64`. Timestamps in `data` are Unix seconds, `meta.timestamp` is Unix
milliseconds. Enumerations are `String`s, and every response struct is
`#[non_exhaustive]`: the API may add a value or a field without breaking your
build. An unknown field in an answer is ignored.

## Errors

Every failure is a `tracefast::Error`:

```rust,no_run
use tracefast::{AddressesPortfolioParams, Client, ErrorKind};

async fn portfolio(client: &Client, address: &str) -> tracefast::Result<()> {
    let params = AddressesPortfolioParams::new().block(30_000_000);
    match client.addresses().portfolio(address, &params).await {
        Ok(res) => println!("{} positions", res.data.positions.len()),
        Err(e) if e.code() == "block_not_available" => {
            println!("covered up to {:?}", e.details().and_then(|d| d.get("maxAvailableBlock")));
        }
        Err(e) if e.reason() == Some("plan_scope_missing") => {
            println!("needs {:?}", e.details().and_then(|d| d.get("minimumPlan")));
        }
        Err(e) if e.kind() == ErrorKind::RateLimit => {
            println!("try again in {:?} (request {:?})", e.retry_after(), e.request_id());
        }
        Err(e) => return Err(e),
    }
    Ok(())
}
```

| Accessor | |
|---|---|
| `status()` | HTTP status; `0` when no answer arrived |
| `code()` | the stable word to branch on: `bad_request`, `invalid_cursor`, `not_found`, `block_not_available`, `coverage_gap`, `rate_limited`, … — or the SDK's own `unexpected_response`, `connection_error`, `timeout`, `invalid_argument` |
| `kind()` | an `ErrorKind`: `BadRequest`, `Authentication`, `PermissionDenied`, `NotFound`, `UnprocessableEntity`, `RateLimit`, `InternalServer`, `Connection`, `Timeout`, `UnexpectedResponse`, `InvalidArgument`, `Other`; shortcuts `is_rate_limited()`, `is_not_found()`, … |
| `reason()` | the stable sub-code: `error.reason` on key, plan and limit refusals (`plan_scope_missing`, …), otherwise `details.reason` |
| `bucket()` | on a `429`, which limit refused |
| `details()` | what to act on: `maxAvailableBlock`, `maxLimit`, `minimumPlan`, `currentPlan`, … |
| `body()` | the raw `error` object, so a field this SDK does not know survives |
| `request_id()` | quote it when you contact support |
| `retry_after()` | the wait from `Retry-After`, exactly as sent |
| `message()` | for humans |

`Display` prints `"<status> <code>: <message>"`. The message may be reworded —
never parse it. An answer that is not the API's (a redirect, a proxy page, a
body that does not match the method's type) is `unexpected_response`; the cause
is attached as the error's `source()`. The codes are listed at
<https://docs.tracefast.xyz/api/conventions#errors>.

## Retries and timeouts

Only answers that say "try again" are retried, at most `max_retries` times:

- `429` with a `Retry-After` of at most `max_retry_wait` — after that wait. A
  `429` with a longer `Retry-After` or without one, and every `quota_exceeded`,
  is returned at once, `retry_after()` telling the real wait;
- `503` — after `Retry-After` (at most `max_retry_wait`), or with exponential
  backoff from 0.5 s with jitter, each wait at most 8 s;
- a connection failure, a body cut off included — with the same backoff.

A timeout is **not** retried, and nothing else is. Every method is a `GET`, so a
retry never repeats a side effect. `timeout` bounds each attempt, the body
included, so the worst case of one call is `(max_retries + 1) × timeout` plus
the waits — with the defaults, 3 × 30 s plus at most 2 × 60 s.

## Pagination

A paged method returns one page, with `data.next_cursor` while there are more.
Two walkers come with it: `<method>_pages` returns whole pages, `meta` included;
`<method>_items` returns the rows one by one. Bound them with `max_pages` and
`max_items`:

```rust,no_run
use tracefast::BlocksTransactionsParams;

async fn walk(client: &tracefast::Client) -> tracefast::Result<()> {
    let params = BlocksTransactionsParams::new().limit(100);

    let mut pages = client.blocks().transactions_pages(25_920_000, &params).max_pages(5);
    while let Some(page) = pages.next_page().await? {
        println!("{} transactions", page.data.transactions.len());
    }

    let mut txs = client.blocks().transactions_items(25_920_000, &params).max_items(250);
    while let Some(tx) = txs.next_item().await? {
        println!("{}", tx.hash);
    }
    Ok(())
}
```

A walker stops when `nextCursor` is absent — a page may hold fewer rows than
`limit` and still not be the last. A cursor that is empty or repeats is an
`unexpected_response` error. `400 invalid_cursor` comes back as an error: start
again from the first page and discard what you collected — for the feeds of one
block it means the block was reorganised. The feeds of an address
(`addresses().transactions`, `token_transfers`, …) need the **Investigation**
plan.

With the `stream` feature the walkers are also `futures_core::Stream`s:

```rust,no_run
#[cfg(feature = "stream")]
async fn rows(client: &tracefast::Client) -> tracefast::Result<usize> {
    use futures::TryStreamExt;

    let params = tracefast::BlocksTransactionsParams::new();
    let rows: Vec<_> = client.blocks().transactions_items(25_920_000, &params).max_items(500).try_collect().await?;
    Ok(rows.len())
}
```

## Types

The response types live in `tracefast::types`, one struct per schema of the API
catalogue, with `serde` `Serialize` and `Deserialize`. Because they are
`#[non_exhaustive]`, build test values with `serde_json`, or start from
`Default` and set fields:

```rust,no_run
use tracefast::types::{Block, ProxyImplementationsResponse};

fn samples() -> serde_json::Result<()> {
    let mut block = Block::default();
    block.number = 20_000_000;

    let _proxy: ProxyImplementationsResponse = serde_json::from_value(serde_json::json!({
        "data": { "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", "chainId": 1, "implementations": [] },
        "meta": { "cached": false, "chain": "ethereum", "docs": "https://docs.tracefast.xyz/api/contract-proxy", "timestamp": 0 }
    }))?;
    Ok(())
}
```

`Client`, `Error`, the walkers and the method futures are `Send` and `Sync`.

## Versioning and stability

The crate is `0.x`: a minor version may break the API. From 1.0 it follows
semantic versioning. Methods marked **Beta** in their documentation are outside
that promise: their shape may still change. See
<https://docs.tracefast.xyz/api/conventions#stability>.

## Support

Open an issue at <https://github.com/datanestllc/tracefast-sdk/issues>. For a
failed call, quote its `request_id()`.

## Development

The code in `src/generated/` and `tests/generated/` is generated by
`codegen/emit-rust.mjs` from the API catalogue snapshot — do not edit it. The
tests read the recorded answers in `../fixtures`, so they run from the
repository, not from the packaged crate.

```bash
node codegen/generate.mjs           # from the repository root
cd rust
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

## License

MIT — see the `LICENSE` file.
