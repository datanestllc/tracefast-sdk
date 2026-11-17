# Changelog

All three packages share one version and one changelog. Dates are UTC.

## 0.1.0 — unreleased (private preview)

First version of the TypeScript, Python and Rust SDKs, generated from API
catalogue `0.1.0+39811dbb`.

- Every method of the catalogue — 109 — typed, grouped in 12 namespaces plus
  `resolve`. All of them are stable.
- Values that can take more than one shape (a decoded argument, the checks of
  a proof) are typed as unions: a TypeScript union, a Python `|` alias, a Rust
  untagged enum.
- The whole `{ data, meta }` envelope; absent, `null` and a value kept apart.
- One error type with kinds, `code`, `reason`, `bucket`, `details`,
  `retryAfter` and `requestId`.
- Retries on `429` with a short `Retry-After`, `503` and connection failures;
  never on a timeout, a long `Retry-After` or `quota_exceeded`.
- Page and row walkers for every paged method, with limits.
- The key is checked at construction, sent only in `x-api-key`, and never
  followed through a redirect.
