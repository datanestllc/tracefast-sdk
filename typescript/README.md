# @tracefast/sdk

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

`@tracefast/sdk` is the official TypeScript and JavaScript SDK for the
[TraceFast API](https://docs.tracefast.xyz/api): every method, typed, for Node.js,
Bun, Deno and edge runtimes.

> **Private preview.** Not on npm yet, and installable only by people with
> access to the private repository. The API may change before 1.0.

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [Quick start](#quick-start)
- [Authentication and plans](#authentication-and-plans)
- [Configuration](#configuration)
- [Methods](#methods)
- [Responses](#responses)
- [Errors](#errors)
- [Retries, timeouts and cancellation](#retries-timeouts-and-cancellation)
- [Pagination](#pagination)
- [Types](#types)
- [Versioning and stability](#versioning-and-stability)
- [Support](#support)
- [Development](#development)

## Requirements

- Node.js 20.19+ or 22.12+, Bun, Deno, or an edge runtime with `fetch` and
  `AbortSignal.any`. ESM; on these Node versions `require()` works too.
- TypeScript 5.0+ if you use the types.
- A TraceFast API key on a paid plan — see [pricing](https://tracefast.xyz/pricing).

## Install

While the package is private, build it from the repository:

```bash
git clone https://github.com/datanestllc/tracefast-sdk.git
cd tracefast-sdk/typescript && npm install && npm pack
# in your project:
npm install /path/to/tracefast-sdk/typescript/tracefast-sdk-0.1.0.tgz
```

Once published: `npm install @tracefast/sdk`.

## Quick start

```ts
import { TraceFast } from '@tracefast/sdk'

const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })

// The newest block.
const { data: block } = await tf.blocks.latest()
console.log(block.number, block.hash)

// Every implementation USDC's proxy has run, newest first.
const { data: proxy } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
for (const impl of proxy.implementations) {
  // supersededBy: an address = replaced; null = still current; absent = storage cannot say.
  const state = impl.supersededBy === null ? 'current' : (impl.supersededBy ?? 'not stated')
  console.log(impl.implementation, 'since block', impl.firstSeenBlock, state)
}

// One transaction.
const { data: tx } = await tf.transactions.get('0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5')
console.log(tx.from, '→', tx.to, tx.valueWei, 'wei')
```

Run it: save as `quickstart.ts` and run `node --experimental-strip-types quickstart.ts`
(Node 22.6+) or `npx tsx quickstart.ts`, with `TRACEFAST_API_KEY` set.

## Authentication and plans

Create a key at [tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
The SDK sends it in the `x-api-key` header and nowhere else.

> The key is a server-side secret. Do not use it in code that runs in a
> browser: anyone who opens the page can read it.

Each method needs a plan — **Enriched Data**, **Intelligence** or
**Investigation** — and every higher plan includes it. The method's doc comment
names the plan. A key whose plan does not include the method is refused:

```ts
import { PermissionDeniedError } from '@tracefast/sdk'

try {
  await tf.contracts.capabilities('0xdac17f958d2ee523a2206206994597c13d831ec7')
} catch (e) {
  if (e instanceof PermissionDeniedError && e.reason === 'plan_scope_missing') {
    console.log('needs', e.details?.minimumPlan, '— this key has', e.details?.currentPlan)
  } else throw e
}
```

## Configuration

```ts
const tf = new TraceFast({
  apiKey: process.env.TRACEFAST_API_KEY!, // required
  timeout: 30_000,                         // ms per attempt, the body included
  maxRetries: 2,
  userAgent: 'my-app/1.4',
})

// A copy with other settings, for one part of a program:
const patient = tf.withOptions({ timeout: 120_000, maxRetries: 4 })
```

| Option | Default | |
|---|---|---|
| `apiKey` | — | your `tf_live_…` key; visible ASCII only — a space or a line break is refused at construction |
| `baseUrl` | `https://tracefast.xyz/api/v1` | must be `https` |
| `chainId` | `1` | Ethereum mainnet, the only chain served today |
| `timeout` | `30000` | milliseconds per attempt, the body included |
| `maxRetries` | `2` | see [Retries](#retries-timeouts-and-cancellation); `0` turns them off |
| `maxRetryWait` | `60` | the longest `Retry-After`, in seconds, the client waits out |
| `userAgent` | — | appended to `tracefast-typescript/<version>` |
| `fetch` | the global `fetch` | any `fetch`-compatible function |

Every method also takes per-call options as its last argument: `signal`,
`timeout` and `maxRetries`.

## Methods

Methods are grouped as in the [API documentation](https://docs.tracefast.xyz/api/methods).
Path parameters are positional; query parameters go in one object, and a
parameter you leave out is not sent.

| Namespace | For example |
|---|---|
| `tf.blocks` | `latest()`, `get(25_920_000)`, `atTime({ timestamp: 1767225600 })`, `transactions(25_920_000, { limit: 100 })` |
| `tf.transactions` | `get(hash)`, `events(hash)`, `failures(hash)`, `storageWrites(hash)` |
| `tf.traces` | `get(hash, { includeCalldata: true })`, `frames(hash, { limit: 100 })` |
| `tf.contracts` | `get(address)`, `proxy(address)`, `bytecode(address)`, `capabilities(address)` |
| `tf.tokens` | `get(token)`, `holders(token)`, `price(token, { block: 20_000_000 })`, `lendingMarkets(token)` |
| `tf.addresses` | `get(address)`, `portfolio(address)`, `attributes(address)`, `transactions(address, { direction: 'sent' })` |
| `tf.pools` | `get(pool)`, `forPair({ tokenA, tokenB })`, `swaps(pool)`, `reserves(pool)` |
| `tf.consensus` | `current()`, `queues()`, `validator(1)`, `missedSlots()` |
| `tf.network` | `daily()`, `gasDaily()`, `topPools({ by: 'volume', days: 7 })`, `topDexTokens()` |
| `tf.lending` | `config('aave_v3')` |
| `tf.signals` | `recent({ kind: 'account_drain' })`, `kinds()`, `rates()` |
| `tf.risk` | `blacklistStatus(address)`, `blacklistEvents(address)` |
| `tf.resolve` | `resolve({ q: 'usdc' })` |

Hover a method in your editor to see what it returns, the plan it needs,
whether it is **Beta**, and a link to its documentation page.

## Responses

A method returns the API's whole answer, `{ data, meta }`:

```ts
const { data, meta } = await tf.blocks.latest()
meta.asOfBlock // the block the answer is true at
meta.docs      // the documentation page of the method
```

What `meta.asOfBlock` means is stated per method; for the at-block methods it
is the horizon of the history, and the point in time is `data.block`.

The types say exactly what the API can send:

- `field?: T` — the field may be **absent**;
- `field?: T | null` — absent, `null` or a value, and they can mean different
  things: for a proxy implementation, `supersededBy` is an address when it was
  replaced, `null` when it is still current, and absent when storage cannot say.

An absent fact is **not proven** — it never means "no".

Units are the API's: wei, gwei and raw token amounts are decimal **strings**
(the unit is in the field name — `valueWei`, `amountGwei`, `amountRaw`);
USD values and decimal-scaled amounts are numbers, for display. Block numbers
and counts are numbers. `meta.timestamp` is Unix milliseconds; timestamps in
`data` and the ones you pass are Unix **seconds**.

Enumerations are open — `'recovered' | 'recovery_failed' | 'not_available' | (string & {})` —
because the API may add a value.

## Errors

Every failure is a `TraceFastError`, and a subclass names its kind:

| Class | When |
|---|---|
| `BadRequestError` | `400` — the request is outside the contract; `reason` says which rule |
| `AuthenticationError` | `401` — no key, or a key that is not valid |
| `PermissionDeniedError` | `403` — the plan does not include the method (`reason: 'plan_scope_missing'`) |
| `NotFoundError` | `404` |
| `UnprocessableEntityError` | `422` — the data cannot answer (`block_not_available`, `coverage_gap`, …) |
| `RateLimitError` | `429` — `bucket` says which limit |
| `InternalServerError` | `5xx` |
| `APIConnectionError` | the connection failed or broke |
| `APITimeoutError` | no complete answer within `timeout` |
| `RequestAbortedError` | your `signal` fired |
| `UnexpectedResponseError` | an answer that is not the API's — a redirect, a proxy page |

```ts
import { UnprocessableEntityError } from '@tracefast/sdk'

try {
  await tf.tokens.price('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', { block: 99_000_000 })
} catch (e) {
  if (e instanceof UnprocessableEntityError && e.code === 'block_not_available') {
    console.log('covered up to', e.details?.maxAvailableBlock)
  } else throw e
}
```

| Property | |
|---|---|
| `status` | HTTP status; `0` when no answer arrived |
| `code` | the stable word to branch on — see [errors](https://docs.tracefast.xyz/api/conventions#errors) |
| `reason` | the stable sub-code, from the error or its `details` |
| `bucket` | on a `429`, which limit refused |
| `details` | what to act on, e.g. `maxAvailableBlock`, `maxLimit`, `minimumPlan` |
| `retryAfter` | seconds from `Retry-After`, exactly as sent |
| `requestId` | quote it when you contact support |
| `body` | the raw error object |

The message is for humans and may be reworded — never parse it.
`TraceFastError.is(e)` also recognises an error from another copy of the package.

## Retries, timeouts and cancellation

Only answers that say "try again" are retried, up to `maxRetries` times:

- `429` with a `Retry-After` of at most `maxRetryWait` seconds;
- `503`, after `Retry-After` or with backoff (0.5 s doubling, at most 8 s);
- a connection that fails or breaks before the answer is complete.

A `429` with a longer wait (a daily quota) or none (a concurrency cap) is
thrown at once, `retryAfter` telling the real wait. **A timeout is not
retried.** The longest one call can take is `(maxRetries + 1) × timeout` plus
the waits. Every method is a read, so a retry never repeats a side effect.

```ts
const controller = new AbortController()
const trace = tf.traces.get(hash, {}, { signal: controller.signal, timeout: 60_000 })
controller.abort() // trace rejects with RequestAbortedError
```

## Pagination

A paged method returns one page and a `nextCursor` while there are more. Two
walkers follow it for you:

```ts
const address = '0xd8da6bf26964af9d7eed9e03e53415d37aa96045'

// Row by row — here, at most 500 transactions.
for await (const tx of tf.addresses.transactionsItems(address, { limit: 100 }, { maxItems: 500 })) {
  console.log(tx.hash)
}

// Page by page, each a whole response with its meta.
for await (const page of tf.blocks.transactionsPages(25_920_000, { limit: 100 }, { maxPages: 3 })) {
  console.log(page.data.transactions.length)
}
```

A walker stops when `nextCursor` is absent — a page may hold fewer rows than
`limit` and still not be the last. If it throws `invalid_cursor`, start again
from the first page and drop what you collected: for the feeds of one block it
means the block was reorganised. An address's own feeds need the
**Investigation** plan.

## Types

Every request and response type is exported, named after its method:

```ts
import type { ContractsProxyResponse, AddressesTransactionsParams } from '@tracefast/sdk'
```

`CATALOGUE_VERSION` names the API catalogue the package was generated from.

## Versioning and stability

Before 1.0, a minor version may change the API of the package. From 1.0 it
follows semantic versioning. Methods marked **Beta** may change shape on the
API side at any time and are outside that promise — see
[stability](https://docs.tracefast.xyz/api/conventions#stability).
Changes are listed in the [changelog](https://github.com/datanestllc/tracefast-sdk/blob/main/CHANGELOG.md).

## Support

Open an issue at [github.com/datanestllc/tracefast-sdk/issues](https://github.com/datanestllc/tracefast-sdk/issues).
For a failed call, include the error's `requestId`.

## Development

The code in `src/generated/` is generated from the API catalogue — do not edit
it; see the [repository README](https://github.com/datanestllc/tracefast-sdk#development).

```bash
npm install
npm run typecheck   # also checks every recorded live answer against the types
npm test            # every method against its recorded answer, plus behaviour tests
npm run build && npm run smoke
```

## License

[MIT](LICENSE)
