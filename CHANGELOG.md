# Changelog

All three packages share one version and one changelog. Dates are UTC.

## 0.1.0 — 2026-11-17

First version of the TypeScript, Python and Rust SDKs, generated from API
catalogue `0.2.0+811c94f8`.

- Every method of the catalogue — 110 — typed, grouped in 12 namespaces plus
  `resolve`. One is Beta: `transactions.storageSlots`, what each storage slot
  a transaction wrote is.
- A transaction, its trace (with calldata) and its trace frames carry the
  call decoded against known function signatures (`decodedCall`), and a
  transaction its whole calldata (`input`).
- Values that can take more than one shape (a decoded argument, the checks of
  a proof and their values) are typed as unions: a TypeScript union, a Python
  `|` alias, a Rust untagged enum. No field of any response is untyped.
- The whole `{ data, meta }` envelope; absent, `null` and a value kept apart.
- One error type with kinds, `code`, `reason`, `bucket`, `details`,
  `retryAfter` and `requestId` — for the API's refusals and for the key
  check's (`auth_failed`, `forbidden`, the `429` limits and `503`
  `service_degraded`) alike.
- Retries on `429` with a short `Retry-After`, `503` and connection failures;
  never on a timeout, a long `Retry-After` or `quota_exceeded`.
- Page and row walkers for every paged method, with limits.
- The key is checked at construction, sent only in `x-api-key`, and never
  followed through a redirect.
