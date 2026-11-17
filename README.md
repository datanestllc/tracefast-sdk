# TraceFast SDKs

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

These are the official clients for the [TraceFast API](https://docs.tracefast.xyz/api),
for TypeScript, Python and Rust.

| Language | Package | Docs | Version |
|---|---|---|---|
| TypeScript / JavaScript | `@tracefast_xyz/sdk` | [typescript/README.md](typescript/README.md) | 0.1 |
| Python | `tracefast` | [python/README.md](python/README.md) | 0.1 |
| Rust | `tracefast` | [rust/README.md](rust/README.md) | 0.1 |

```bash
npm install @tracefast_xyz/sdk    # TypeScript / JavaScript
pip install tracefast         # Python
cargo add tracefast           # Rust
```

## Before you start

- **A key.** Create one at [tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
  API keys need a paid plan — see [pricing](https://tracefast.xyz/pricing).
- **A plan per method.** Each method needs **Enriched Data**, **Intelligence**
  or **Investigation**; every higher plan includes it. Each method's doc comment
  names its plan.
- **Server-side only.** The key is a secret. Do not use it in code that runs in
  a browser — anyone who opens the page can read it.

## A taste

```ts
import { TraceFast } from '@tracefast_xyz/sdk'

const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })
const { data } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
console.log(data.implementations.length, 'implementations')
```

```python
import os
from tracefast import TraceFast

with TraceFast(api_key=os.environ["TRACEFAST_API_KEY"]) as tf:
    block = tf.blocks.latest()["data"]
    print(block["number"], block["hash"])
```

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tf = tracefast::Client::new(std::env::var("TRACEFAST_API_KEY")?)?;
    let latest = tf.blocks().latest().await?;
    println!("block {}", latest.data.number);
    Ok(())
}
```

Each package's README has a complete quick start and says how to run it.

## What all three share

The packages are generated from one snapshot of the API catalogue by one
generator, and follow one behaviour contract, [SPEC.md](SPEC.md):

- **Every method, typed**, grouped as in the API documentation —
  `addresses`, `blocks`, `consensus`, `contracts`, `lending`, `network`,
  `pools`, `risk`, `signals`, `tokens`, `traces`, `transactions`, plus
  `resolve`.
- **The whole answer.** A method returns `{ data, meta }`; `meta.asOfBlock`
  says which block the answer is true at.
- **Honest types.** Absent, `null` and a value are kept apart. An absent fact is
  *not proven* — never "no".
- **Errors you can branch on.** One error type with kinds (`RateLimitError`,
  `PermissionDeniedError`, …), the stable `code` and `reason`, and the
  `requestId` to quote to support.
- **Careful retries.** Only answers that say "try again" are retried; a quota
  that resets hours away is returned at once.
- **Pagination.** Walk pages or rows, with a limit.
- **Safe by default.** The key goes only in its header — never in a URL, a log
  or an error — and a redirect is never followed.
- **Small footprint.** No runtime dependencies in TypeScript (built on
  `fetch`); `httpx` and `typing-extensions` in Python; `reqwest`, `serde` and
  `tokio` in Rust. No telemetry, no caching.

## Versioning and stability

Before 1.0, a minor version may change a package's API. From 1.0 the packages
follow semantic versioning. Methods marked **Beta** may change shape on the API
side and are outside that promise — see
[stability](https://docs.tracefast.xyz/api/conventions#stability). See the
[changelog](CHANGELOG.md).

## Repository layout

```
openapi/      the catalogue snapshot the SDKs are generated from
codegen/      the generator: catalogue → TypeScript, Python, Rust
fixtures/     one recorded live answer per method, shared by all three test suites
typescript/   @tracefast_xyz/sdk
python/       tracefast (PyPI)
rust/         tracefast (crates.io)
SPEC.md       the behaviour every SDK follows
```

## Development

```bash
node codegen/generate.mjs            # regenerate all three SDKs from openapi/
node codegen/generate.mjs --check    # the generated code is up to date
node codegen/lint.mjs                # no internal names, hosts, secrets, or non-English text
```

Generation reads only the committed snapshot — no network. Each package's
README says how to build and test it. Every test suite checks each recorded
live answer strictly against the catalogue: a field the catalogue does not
describe, a missing required field, or a `null` where none is allowed fails the
build. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Links

- [TraceFast](https://tracefast.xyz) — the Ethereum analytics and forensics app
- [API documentation](https://docs.tracefast.xyz/api) and [every method on one page](https://docs.tracefast.xyz/api/methods)
- [Using TraceFast with AI](https://docs.tracefast.xyz/api/using-ai)
- [Pricing](https://tracefast.xyz/pricing)
- [Blog](https://blog.tracefast.xyz) — investigations and release notes

## Support and security

Questions and bugs: [issues](https://github.com/datanestllc/tracefast-sdk/issues);
for a failed call, include the error's request id. Security reports: see
[SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
