# @tracefast/sdk

The official TypeScript SDK for the [TraceFast API](https://docs.tracefast.xyz/api):
Ethereum blocks, transactions, execution traces, tokens, contracts, DEX pools,
the consensus layer, and what TraceFast proves about an address — now and at
any past block.

- Every one of the API's 97 methods, fully typed from the API catalogue
- The whole answer — `data` and `meta` (`asOfBlock`, `coverage`)
- One error type with stable codes, retries that follow `Retry-After`
- Async iterators over paged methods
- Zero runtime dependencies — built on `fetch`

> **Private preview.** Not on npm yet; the API may still change before 1.0.

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Methods](#methods)
- [Responses](#responses)
- [Errors](#errors)
- [Retries, timeouts and cancellation](#retries-timeouts-and-cancellation)
- [Pagination](#pagination)
- [Types](#types)
- [Runtimes](#runtimes)
- [Development](#development)

## Install

While the package is private, build it from this repository:

```bash
git clone https://github.com/datanestllc/tracefast-sdk.git
cd tracefast-sdk/typescript && npm install && npm run build && npm pack
# in your project:
npm install /path/to/tracefast-sdk/typescript/tracefast-sdk-0.1.0.tgz
```

Once published it will be `npm install @tracefast/sdk`.

## Quick start

```ts
import { TraceFast } from '@tracefast/sdk'

const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })

// The newest block.
const { data: block } = await tf.blocks.latest()
console.log(block.number, block.hash)

// What a contract can do — and what has already been done with it.
const { data: caps } = await tf.contracts.capabilities('0xdac17f958d2ee523a2206206994597c13d831ec7')
for (const c of caps.capabilities) {
  console.log(c.power, { inCode: !!c.inCode, availableNow: !!c.availableNow, usedBefore: !!c.usedBefore })
}

// Resolve a name to the addresses it can mean.
const { data: found } = await tf.resolve({ q: 'usdc' })
```

Get a key at [tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
Every method needs a plan — **Enriched Data**, **Intelligence** or
**Investigation**; each method's doc comment names it.

> The API key is a server-side secret. Do not use this package with a real key
> in code that runs in a browser: anyone who opens the page can read the key.

## Configuration

```ts
const tf = new TraceFast({
  apiKey: process.env.TRACEFAST_API_KEY!, // required
  timeout: 30_000,                         // per call, in ms (default 30 s)
  maxRetries: 2,                           // see "Retries" (default 2)
  userAgent: 'my-app/1.4',                 // appended to the User-Agent header
  // baseUrl: 'https://tracefast.xyz/api/v1', chainId: 1, fetch: customFetch
})
```

| Option | Default | |
|---|---|---|
| `apiKey` | — | your `tf_live_…` key; sent in the `x-api-key` header only |
| `baseUrl` | `https://tracefast.xyz/api/v1` | |
| `chainId` | `1` | Ethereum mainnet, the only chain served today |
| `timeout` | `30000` | milliseconds, per call; overridable per call |
| `maxRetries` | `2` | `0` turns retries off |
| `userAgent` | — | appended to `tracefast-typescript/<version>` |
| `fetch` | `globalThis.fetch` | any `fetch`-compatible function |

## Methods

Methods are grouped as in the API documentation. Path parameters are
positional; query parameters go in one object, and a parameter you leave out is
not sent.

| Namespace | Methods | For example |
|---|---|---|
| `tf.addresses` | 22 | `portfolio(address, { block })`, `transactions(address, { direction, limit })`, `attributes(address)`, `relationHistory(address)` |
| `tf.blocks` | 10 | `latest()`, `get(number)`, `atTime({ timestamp })`, `transactions(number, { limit })` |
| `tf.transactions` | 10 | `get(hash)`, `events(hash)`, `failures(hash)`, `storageWrites(hash)` |
| `tf.traces` | 2 | `get(hash)`, `frames(hash, { limit })` |
| `tf.contracts` | 19 | `get(address)`, `proxy(address)`, `capabilities(address)`, `decompilation(address)` |
| `tf.tokens` | 8 | `get(token)`, `holders(token)`, `price(token, { block })`, `transfers(token)` |
| `tf.pools` | 9 | `get(pool)`, `forPair({ tokenA, tokenB })`, `swaps(pool)`, `reserves(pool)` |
| `tf.consensus` | 11 | `current()`, `queues()`, `validator(index)`, `missedSlots()` |
| `tf.risk` | 5 | `blacklistStatus(address)`, `blacklistEvents(address)` |
| `tf.resolve` | 1 | `resolve({ q })` |

Every method's doc comment carries the method's description, the plan it needs,
whether it is **Beta**, and a link to its page on
[docs.tracefast.xyz](https://docs.tracefast.xyz/api/methods) — your editor shows
all of it on hover.

## Responses

A method returns the API's whole answer:

```ts
const { data, meta } = await tf.addresses.attributesAtBlock(address, { block: 20_000_000 })
meta.asOfBlock // the block the answer is true at
meta.docs      // the documentation page of the method
```

The types say exactly what the API can send:

- `field?: T` — the field may be **absent**;
- `field: T | null` — the field may be **null**;
- `field?: T | null` — both, and they mean different things. For a proxy
  implementation, `supersededBy` is an address when it was replaced, `null` when
  it is still current, and absent when storage cannot say.

An absent fact is **not proven** — it never means "no". Amounts, balances and
wei are decimal strings (they exceed 2^53); block numbers and counts are numbers.

Enumerations are open: `"stable" | "beta" | (string & {})` — the API may add a
value, and your code should expect one.

## Errors

Every failure is a `TraceFastError`:

```ts
import { TraceFast, TraceFastError } from '@tracefast/sdk'

try {
  await tf.addresses.portfolio(address, { block: 30_000_000 })
} catch (e) {
  if (e instanceof TraceFastError && e.code === 'block_not_available') {
    console.log('covered up to', e.details?.maxAvailableBlock)
  } else throw e
}
```

| Property | |
|---|---|
| `status` | HTTP status; `0` when no answer arrived |
| `code` | the stable word to branch on: `bad_request`, `not_found`, `block_not_available`, `rate_limited`, `quota_exceeded`, … — or the SDK's own `unexpected_response`, `connection_error`, `timeout` |
| `reason` | `details.reason`, the stable sub-code, when given |
| `details` | what to act on, e.g. `maxAvailableBlock`, `maxLimit` |
| `requestId` | quote it when you contact support |
| `message` | for humans — it may be reworded, never parse it |

The full list of codes is on [conventions](https://docs.tracefast.xyz/api/conventions#errors).

## Retries, timeouts and cancellation

Only answers that say "try again" are retried, up to `maxRetries` times:

- `429` **with** `Retry-After` — after the time it states (at most 60 s);
- `503` — after `Retry-After`, or with exponential backoff and jitter;
- a connection failure before any answer.

A `429` without `Retry-After` (a daily quota, a concurrency cap) and every other
error are thrown at once. Every method is a read, so a retry never repeats a
side effect.

```ts
const controller = new AbortController()
// Options are the last argument, after the query parameters.
await tf.traces.get(hash, { includeCalldata: true }, { signal: controller.signal, timeout: 60_000 })
```

## Pagination

Paged methods return one page and a `nextCursor` while there are more. The
`…Pages` companion walks them all:

```ts
for await (const page of tf.addresses.transactionsPages(address, { limit: 100 })) {
  for (const tx of page.data.transactions) console.log(tx.hash)
}
```

It stops when `nextCursor` is absent — a page may hold fewer rows than `limit`
and still not be the last. Each page is a whole response, `meta` included.

## Types

Every request and response type is exported:

```ts
import type { ProxyImplementationsResponse, AddressesTransactionsParams } from '@tracefast/sdk'
```

## Runtimes

Node.js 20.3 or later, Deno, Bun, and edge runtimes with `fetch` and
`AbortSignal.any`. ESM only.

## Development

The code in `src/generated/` is generated from the API catalogue — do not edit
it; see the [repository README](../README.md).

```bash
npm install
npm run typecheck
npm test        # every method against its recorded live answer, plus behaviour tests
npm run build   # dist/
```

## License

[MIT](LICENSE)
