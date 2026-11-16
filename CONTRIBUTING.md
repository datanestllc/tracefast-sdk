# Contributing

## How the repository works

The three SDKs are generated. `codegen/ir.mjs` turns the catalogue snapshot in
`openapi/` into one model; `codegen/emit-ts.mjs`, `emit-python.mjs` and
`emit-rust.mjs` turn the model into code. Never edit a file under a
`generated/` directory — change the generator and run it.

Behaviour that is not generated (the HTTP client, errors, retries, pagination)
is hand-written in each package and follows [SPEC.md](SPEC.md). A change of
behaviour is a change of SPEC.md and of all three packages in one pull request.

## Before you open a pull request

```bash
node codegen/generate.mjs && node codegen/generate.mjs --check
node codegen/lint.mjs
```

and the tests of every package you touched (each README lists the commands).
All text — code, comments, docs, test names — is in English.

## Maintainer tasks

These need access the public repository does not give:

- `node codegen/sync.mjs` refreshes the catalogue snapshot. The stability of
  each method comes from a source given in `TRACEFAST_STABILITY_SOURCE`.
- `node fixtures/capture.mjs` re-records one live answer per method from the
  base URL in `TRACEFAST_FIXTURE_BASE`.

After either, regenerate and run every test suite.
