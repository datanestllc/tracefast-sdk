# Changelog

All three packages share one version and one changelog. Dates are UTC.

## 0.1.0 — unreleased (private preview)

First version of the TypeScript, Python and Rust SDKs, generated from API
catalogue `0.1.0+ebace7d7`.

- Every method of the catalogue, typed, grouped by namespace.
- The whole `{ data, meta }` envelope; absent, `null` and a value kept apart.
- One error type with kinds, `code`, `reason`, `bucket`, `details`,
  `retryAfter` and `requestId`.
- Retries on `429` with a short `Retry-After`, `503` and connection failures;
  never on a timeout, a long `Retry-After` or `quota_exceeded`.
- Page and row walkers for every paged method, with limits.
- The key is checked at construction, sent only in `x-api-key`, and never
  followed through a redirect.
