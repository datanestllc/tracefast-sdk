# How every TraceFast SDK behaves

This is the contract the TypeScript, Python and Rust packages share. Each one is
generated from the same snapshot of the published API catalogue
(`openapi/openapi.json`) by the same generator (`codegen/`), and each one follows
the rules below. A rule here changes in all three packages at once, or not at
all.

Sources of truth: the published catalogue
(`https://tracefast.xyz/api/v1/openapi.json`) for operations and shapes; the
TraceFast API documentation (`https://docs.tracefast.xyz/api/conventions`) for
the envelope, errors and pagination.

## 1. The client

- One client object holds the configuration: `apiKey` (required), `baseUrl`
  (default `https://tracefast.xyz/api/v1`), `chainId` (default `1`, the only
  chain served today), `timeout` (default 30 s), `maxRetries` (default 2) and a
  `userAgent` suffix.
- The key travels in the `x-api-key` header and nowhere else — never in the URL.
  The client never logs it and never puts it into an error message.
- A redirect is never followed: a `3xx` answer is an error
  (`unexpected_response`), so the key never travels to another host.
- The key is for server-side code. The SDKs do not support running in a browser
  with a real key, and the README says so.
- Every request sends `User-Agent: tracefast-<language>/<sdk version>` plus the
  configured suffix, and `Accept: application/json`.

## 2. Operations

- Every operation of the catalogue is one method, grouped by its catalogue tag:
  `client.addresses`, `client.blocks`, `client.consensus`, `client.contracts`,
  `client.pools`, `client.risk`, `client.tokens`, `client.traces`,
  `client.transactions`; the one operation tagged `resolve` is `client.resolve`.
- The method name is the `operationId` without `get` and without the tag's noun
  (`getContractCapabilities` → `contracts.capabilities`); overrides live in
  `codegen/names.json`. Python and Rust use snake_case.
- Path parameters are positional, in path order; `chainId` is never a
  parameter — it comes from the client. Query parameters go in one options
  object (TypeScript), keyword arguments (Python) or a params struct (Rust).
- **A query parameter that is not set is not sent.** The API refuses unknown and
  duplicate keys (`400 bad_request`); the SDK never adds one of its own.
- Path values are percent-encoded. Addresses and hashes are passed as given; the
  API accepts any case and answers in lowercase.
- Each method carries the catalogue's summary and description as its doc
  comment, the link to its documentation page (`externalDocs`), the minimum
  plan, and **Beta** when the operation is in beta.

## 3. Responses

- A method returns the whole envelope, `{ data, meta }` — never `data` alone.
  `meta.asOfBlock`, `meta.coverage` and `meta.cached` are part of the answer.
- **Absent, `null` and a value are three different answers** and the types keep
  them apart:
  - a field not in the schema's `required` list may be absent;
  - a field whose type admits `null` may be `null`;
  - a field that is both may be either, and the two mean different things
    (`supersededBy`: absent = storage cannot say, `null` = not replaced).
  TypeScript: `field?: T | null`. Python: `NotRequired[T | None]` in a
  `TypedDict` — the dict keeps the key absent. Rust: `Option<T>` for one of the
  two, `Option<Option<T>>` (absent / `null` / value) for both.
- **Types are open.** An unknown field in a response is ignored, never an error.
  An enum accepts values it does not know (the catalogue says "other values may
  appear"): TypeScript `"a" | "b" | (string & {})`, Python `Literal[...] | str`,
  Rust a `String`.
- Amounts, balances and wei are JSON strings in the API and stay strings in the
  SDK. Block heights, counts and timestamps are integers (TypeScript `number` —
  all of them are below 2^53).

## 4. Errors

- A non-2xx answer becomes one error type, `TraceFastError`, with `status` (HTTP
  status), `code` (the stable word from the error registry), `message` (for
  humans — never parse it), `details` (the object, when present — `details.reason`
  is the stable sub-code) and `requestId` (quote it to support).
- A body that is not the error envelope (a proxy page, a timeout), and a `2xx`
  answer whose body is not JSON, still become `TraceFastError`, with `code`
  `unexpected_response` and the status.
- A network failure or a timeout becomes `TraceFastError` with `status` `0` and
  `code` `connection_error` or `timeout`, the cause attached.
- Branch on `code` and `details.reason`, never on `message`.

## 5. Retries

Only answers that say "try again" are retried, at most `maxRetries` times:

- `429` **with** a `Retry-After` header — wait that many seconds (capped at 60)
  and retry. A `429` without `Retry-After` (a daily quota, a concurrency cap) is
  returned at once.
- `503` (`service_unavailable`, a transient limit on our side) — wait
  `Retry-After` when present, otherwise exponential backoff from 0.5 s with
  jitter.
- A connection error before any response — backoff as above.

Everything else (`400`, `401`, `402`, `403`, `404`, `422`, `500`) is returned at
once. Every method is a `GET`, so a retry never repeats a side effect.

## 6. Pagination

- An operation with a `cursor` query parameter is paged. `data.nextCursor` is
  present while there are more rows and **absent** on the last page.
- Besides the plain method, a paged operation gets a pager that walks the pages:
  TypeScript `…Pages()` (an async iterator of responses), Python `…_pages()`
  (a generator of responses), Rust `…_pages()` (a pager with `next_page()`).
- The pager stops when `nextCursor` is absent — never on page length: a page may
  hold fewer than `limit` rows and still have a next one.
- The pager passes the cursor back unchanged, with the same other parameters.
  `400 invalid_cursor` surfaces as an error; the caller restarts.

## 7. What the SDK does not do

- No caching, no client-side validation of addresses or ranges beyond types —
  the API is the judge and its refusal says why.
- No telemetry. The SDK talks to `baseUrl` and nothing else.
- No other channel: paying per call (x402) and MCP are not part of the SDK yet.
