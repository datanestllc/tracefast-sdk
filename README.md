# TraceFast SDKs

Official clients for the [TraceFast API](https://docs.tracefast.xyz/api) —
Ethereum blocks, transactions, execution traces, tokens, contracts, DEX pools,
the consensus layer, and what TraceFast proves about an address, now and at any
past block.

| Language | Package | Directory | Status |
|---|---|---|---|
| TypeScript / JavaScript | `@tracefast/sdk` | [`typescript/`](typescript) | private preview |
| Python | `tracefast` | [`python/`](python) | private preview |
| Rust | `tracefast` | [`rust/`](rust) | private preview |

> **Private preview.** The packages are not published to npm, PyPI or
> crates.io yet, and their API may still change. Install them from this
> repository (see each package's README).

All three are generated from the same snapshot of the API catalogue and follow
one behaviour contract, [`SPEC.md`](SPEC.md): the same method names, the same
errors, the same retries, the same pagination.

## A taste

```ts
import { TraceFast } from '@tracefast/sdk'

const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })

// Every implementation USDC's proxy has run, newest first.
const { data, meta } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
for (const impl of data.implementations) {
  // supersededBy: an address = replaced; null = still current; absent = storage cannot say.
  const state = impl.supersededBy === null ? 'current' : (impl.supersededBy ?? 'not stated')
  console.log(impl.implementation, impl.firstSeenBlock, state)
}
```

```python
import os
from tracefast import TraceFast

with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
    for page in tf.addresses.transactions_pages("0xd8da6bf26964af9d7eed9e03e53415d37aa96045", limit=100):
        for tx in page["data"]["transactions"]:
            print(tx["hash"])
```

```rust
let tf = tracefast::Client::new(std::env::var("TRACEFAST_API_KEY")?)?;
let latest = tf.blocks().latest().await?;
println!("block {} at {}", latest.data.number, latest.data.timestamp);
```

## What you get

- **Every method of the API, typed.** 97 operations in ten groups —
  `addresses`, `blocks`, `consensus`, `contracts`, `pools`, `risk`, `tokens`,
  `traces`, `transactions`, plus `resolve`. Each method carries its description,
  the plan it needs and a link to its documentation page.
- **The whole answer.** A method returns `{ data, meta }`: `meta.asOfBlock`
  tells you which block the answer is true at, `meta.coverage` the bounds of
  what the method can state.
- **Honest types.** A field the API may leave out is optional; a field that may
  be `null` admits `null`; where both can happen they mean different things and
  the types keep them apart. An absent fact is *not proven* — never "no".
- **One error type.** `TraceFastError` with the stable `code`, `details.reason`
  and the `requestId` to quote to support.
- **Retries that respect the API.** Only answers that say "try again" are
  retried: `429` with `Retry-After`, `503`, and connection failures.
- **Pagination.** Every paged method has a `…Pages` / `…_pages` companion that
  follows `nextCursor` to the end.
- **No surprises.** No telemetry, no caching, no dependency beyond an HTTP
  client. The SDK talks to `https://tracefast.xyz/api/v1` and nothing else.

## Before you start

You need a TraceFast API key — create one at
[tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
Each method needs a plan (**Enriched Data**, **Intelligence** or
**Investigation**); the method's doc comment and
[documentation page](https://docs.tracefast.xyz/api/methods) name it.

The key is a server-side secret. **Do not use it in code that runs in a
browser** — anyone who opens the page can read it.

Methods marked **Beta** may still change shape; stable methods are frozen — see
[stability](https://docs.tracefast.xyz/api/conventions#stability).

## Repository layout

```
openapi/      the catalogue snapshot the SDKs are generated from
codegen/      the generator: catalogue → TypeScript, Python, Rust
fixtures/     one live answer per operation, shared by the tests of all three
typescript/   @tracefast/sdk
python/       tracefast (PyPI)
rust/         tracefast (crates.io)
SPEC.md       the behaviour every SDK follows
```

## Development

```bash
node codegen/generate.mjs            # regenerate all three SDKs from openapi/
node codegen/generate.mjs --check    # CI: the generated code is up to date
node codegen/lint.mjs                # CI: no internal names, hosts or secrets
```

Generation reads only the committed snapshot — no network. To move to a newer
catalogue, refresh the snapshot (`node codegen/sync.mjs`), regenerate, re-take
the fixtures (`node fixtures/capture.mjs`) and run every package's tests. Each
package's README says how to build and test it.

A test fails if a live answer has a field the catalogue does not describe, lacks
one it requires, or is `null` where the catalogue does not allow it — so a gap
between the catalogue and the API is caught before a release.

## License

[MIT](LICENSE)
