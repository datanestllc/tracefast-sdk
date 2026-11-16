# Security

## Reporting a vulnerability

Please do not open a public issue for a security problem. Write to
[security@tracefast.xyz](mailto:security@tracefast.xyz) — the address TraceFast
publishes for this in its `security.txt`.

## How the SDKs handle your key

- The key is sent only in the `x-api-key` header, only to the configured base
  URL, which must use `https`.
- A redirect is never followed, so the key never reaches another host.
- The key is checked when the client is created; it never appears in an error,
  a log line, a `repr` or a `Debug` output.
- The SDKs send no telemetry and talk to no other host.

The key is a server-side secret. Do not use it in code that runs in a browser.
If a key may have leaked, revoke it at
[tracefast.xyz/settings/api/keys](https://tracefast.xyz/settings/api/keys).
