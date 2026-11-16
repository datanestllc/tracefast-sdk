# How every TraceFast SDK behaves

This is the contract the TypeScript, Python and Rust packages share. Each one is
generated from the same snapshot of the published API catalogue
(`openapi/openapi.json`) by the same generator (`codegen/`), and each one follows
the rules below. A rule here changes in all three packages at once, or not at
all.

Sources of truth: the published catalogue
(`https://tracefast.xyz/api/v1/openapi.json`) for operations and shapes; the
TraceFast API documentation (`https://docs.tracefast.xyz/api/conventions` and
`https://docs.tracefast.xyz/api/authentication`) for the envelope, errors,
refusals and pagination.

## 1. The client

- One client object holds the configuration: `apiKey` (required), `baseUrl`
  (default `https://tracefast.xyz/api/v1`), `chainId` (default `1`, the only
  chain served today), `timeout` (default 30 s), `maxRetries` (default 2),
  `maxRetryWait` (default 60 s) and a `userAgent` suffix.
- `withOptions(...)` returns a copy of the client with some of these changed —
  for one call or one part of a program — sharing the connection pool.
- **The key** travels in the `x-api-key` header and nowhere else — never in the
  URL. The client never logs it and never puts it into an error, a `repr`, a
  `Debug` output or an error's cause.
  - At construction the key is checked: it must be non-empty and made only of
    visible ASCII characters (`0x21`–`0x7E`). A key with a space, a line break
    or a typographic character is refused there, with a configuration error
    whose message does not repeat the key.
- **Redirects are never followed**, whatever HTTP client the caller supplies: a
  `3xx` answer is an error (`unexpected_response`), whatever its body, so the
  key never travels to another host.
- `baseUrl` must be `https`, except for a loopback host (tests).
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
  Integer path parameters are integers (TypeScript also accepts a decimal
  string).
- **A query parameter that is not set is not sent.** The API refuses unknown and
  duplicate keys (`400 bad_request`); the SDK never adds one of its own.
- Path values are percent-encoded. A path value that is empty, `.` or `..` is
  refused before any request with an argument error — TypeScript `TypeError`,
  Python `ValueError`, Rust `Error` of kind `InvalidArgument` (`code`
  `invalid_argument`): URL normalisation would otherwise send the call to a
  different endpoint. Beyond that the SDK does not
  validate values — the API is the judge, and its refusal says why.
- Addresses and hashes are passed as given; the API accepts any case and answers
  in lowercase.
- Each method carries the catalogue's summary and description as its doc
  comment, the minimum plan, **Beta** when the operation is in beta, and the link
  to its documentation page (`externalDocs`).

## 3. Responses

- A method returns the whole envelope, `{ data, meta }` — never `data` alone.
  `meta.asOfBlock`, `meta.coverage` and `meta.cached` are part of the answer.
  What `asOfBlock` means is stated per method: for most it is the block the
  answer is true at; for the at-block methods it is the horizon of the history,
  and the point in time is `data.block`.
- **Absent, `null` and a value are three different answers** and the types keep
  them apart:
  - a field not in the schema's `required` list may be absent;
  - a field whose type admits `null` may be `null`;
  - a field that is both may be either, and the two mean different things
    (`supersededBy`: absent = storage cannot say, `null` = not replaced).
  TypeScript: `field?: T | null`. Python: `NotRequired[T | None]` in a
  `TypedDict` — the dict keeps the key absent. Rust: `Option<T>` for one of the
  two, `Option<Option<T>>` (absent / `null` / value) for both.
  The types follow the catalogue strictly. Today the catalogue marks many fields
  nullable that never carry `null`; the API team is narrowing that, and the types
  narrow with it.
- An absent fact is **not proven** — never "no".
- **Types are open.** An unknown field in a response is ignored, never an error.
  An enum accepts values it does not know (the catalogue says "other values may
  appear"): TypeScript `"a" | "b" | (string & {})`, Python `Literal[...] | str`,
  Rust a `String`. In Rust, response structs are `#[non_exhaustive]`, so a new
  field is not a breaking change.
- Units are the API's:
  - wei, gwei and raw token amounts are decimal **strings**, the unit in the field
    name or description (`balanceWei`, `amountGwei`, `reserveRaw` — which may be
    negative). Decimal-scaled token amounts and USD values are floats, for
    display.
  - Block heights and counts are integers (TypeScript `number` — all below 2^53).
  - `meta.timestamp` is Unix **milliseconds**; timestamps in `data` are Unix
    **seconds**, or RFC 3339 strings where the field says so. Timestamps you pass
    (`timestamp`, `from`, `to`) are Unix **seconds**.
  - `?block=N` means the state at the end of block N; absent means now.

## 4. Errors

- Every failure is a `TraceFastError` (in Rust, `tracefast::Error`), with:
  - `status` — the HTTP status; `0` when no answer arrived;
  - `code` — the stable word to branch on: the API's registry (`bad_request`,
    `invalid_cursor`, `not_found`, `chain_not_supported`, `block_not_available`,
    `coverage_gap`, `quota_exceeded`, `service_unavailable`, `internal_error`, …),
    the gateway's (`auth_failed`, `forbidden`, `rate_limited`,
    `concurrency_limited`), or the SDK's own (`unexpected_response`,
    `connection_error`, `timeout`, `aborted`, and in Rust `invalid_argument`);
  - `reason` — the stable sub-code: `error.reason` on the gateway's key, plan and
    limit refusals (`api_key_required`, `plan_scope_missing`, …), otherwise
    `details.reason`;
  - `bucket` — on a `429`, which limit refused (`base`, `class:…`,
    `concurrency`, `quota_daily`, `quota_method`, …);
  - `details` — what to act on, when present (`maxAvailableBlock`, `maxLimit`,
    `minimumPlan`, `currentPlan`, `upgrade`, …);
  - `requestId` — from the body, else the `X-Request-Id` header; quote it to
    support;
  - `retryAfter` — seconds from `Retry-After`, exactly as sent (not capped);
  - `body` — the raw `error` object, so a field the SDK does not know survives;
  - `message` — `"<status> <code>: <the API's message>"`, for humans. Never
    parse it.
- Errors are also typed by kind. In TypeScript and Python, subclasses of
  `TraceFastError`: `BadRequestError` (400), `AuthenticationError` (401),
  `PermissionDeniedError` (403), `NotFoundError` (404),
  `UnprocessableEntityError` (422), `RateLimitError` (429),
  `InternalServerError` (5xx), `APIConnectionError` (`connection_error`),
  `APITimeoutError` (`timeout`), `UnexpectedResponseError`; TypeScript adds
  `RequestAbortedError` (`aborted`). In Rust, `Error::kind()` returns the same
  set as a `#[non_exhaustive]` enum, with `is_…()` shortcuts.
- A body that is not the error envelope (a proxy page), a `2xx` answer whose
  body is not JSON or does not match, and any `3xx`, become
  `unexpected_response` with the status.
- A failure while connecting, sending, or reading the body becomes
  `connection_error` (status `0` if no status line arrived) or `timeout`, the
  cause attached — never a raw runtime error. In TypeScript, a caller's abort
  signal becomes `aborted`. In Python async code, task cancellation is not
  caught: `CancelledError` propagates as it must.
- Branch on `code`, `reason` and the error kind, never on `message`.

## 5. Retries

Only answers that say "try again" are retried, at most `maxRetries` times:

- `429` with a `Retry-After` of at most `maxRetryWait` — wait that long, then
  retry. A `429` with a longer `Retry-After` (a daily or per-method quota resets
  hours away) or without one (a concurrency cap), and every `quota_exceeded`,
  are returned at once, `retryAfter` telling the real wait.
- `503` — wait `Retry-After` when present (at most `maxRetryWait`), otherwise
  exponential backoff from 0.5 s with jitter, each wait at most 8 s.
- `connection_error` — backoff as above. A timeout is **not** retried.

Everything else is returned at once. Every method is a `GET`, so a retry never
repeats a side effect. `timeout` bounds each attempt, the body included
(Python: each phase of an attempt, as httpx does); the worst case of one call is
`(maxRetries + 1) × timeout` plus the waits.

## 6. Pagination

- An operation with a `cursor` query parameter is paged. `data.nextCursor` is
  present while there are more rows and **absent** on the last page.
- Besides the plain method, a paged operation gets two walkers:
  - **pages** — TypeScript `…Pages()`, Python `…_pages()`, Rust `…_pages()`:
    every whole response, `meta` included;
  - **items** — TypeScript `…Items()`, Python `…_items()`, Rust `…_items()`: the
    rows of the operation's row array (`transactions`, `holders`, …; named in
    `codegen/names.json` where `data` has more than one array), one by one.
  Both accept a limit on how much they walk (`maxPages`, `maxItems`).
- A walker stops when `nextCursor` is absent — never on page length: a page may
  hold fewer than `limit` rows and still have a next one. A `nextCursor` that is
  empty or repeats the previous one is `unexpected_response`.
- The walker passes the cursor back unchanged, with the same other parameters.
  `400 invalid_cursor` surfaces as an error; restart from the first page and
  discard what was collected — for the feeds of one block it means the block was
  reorganised, and the rows already read belong to the old block.

## 7. What the SDK does not do

- No caching, no client-side validation of values beyond types and the path
  rule above.
- No telemetry. The SDK talks to `baseUrl` and nothing else.
- One channel: the REST API with an API key.
