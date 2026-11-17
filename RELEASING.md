# Releasing

All three packages — npm `@tracefast_xyz/sdk`, PyPI `tracefast`, crates.io
`tracefast` — share one version and are released together by
[`.github/workflows/release.yml`](.github/workflows/release.yml) when a tag
`v<version>` is pushed.

## Each release

1. Refresh the catalogue and the fixtures if the API changed
   ([CONTRIBUTING.md](CONTRIBUTING.md#maintainer-tasks)), regenerate, and run
   every test suite.
2. Set the new version in all four places:
   - `typescript/package.json` and `typescript/src/version.ts`;
   - `python/src/tracefast/_version.py`;
   - `rust/Cargo.toml` (then `cargo update -p tracefast --offline` in `rust/`).
3. Give the version a dated heading in `CHANGELOG.md`: `## 0.1.0 — 2026-11-18`.
4. Commit, wait for CI to pass on `main`, then tag and push the tag:

   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

The workflow runs the full CI again, refuses a tag that does not match every
manifest and the changelog, and then publishes to the three registries in
parallel. If one registry fails, re-run only that job: the others are already
published, and a registry never accepts the same version twice.

## Once: registry setup

No credential is stored in the repository. Every registry trusts the workflow
by OIDC (trusted publishing) for repository `datanestllc/tracefast-sdk`,
workflow `release.yml`, environment `release`.

1. **GitHub.** Create the environment `release` (Settings → Environments). Add
   yourself as a required reviewer, so that no publish starts without an
   approval, and allow deployments from tags `v*` only.
2. **PyPI.** On the account that will own `tracefast`, add a *pending* trusted
   publisher for project `tracefast` with the values above. A pending
   publisher works before the project exists, so no token is ever needed.
3. **npm.** Create the organisation `tracefast_xyz` (it owns the scope
   `@tracefast_xyz`). npm can only trust a package that exists, so the first
   version is published with a granular access token: store it as the secret
   `NPM_TOKEN` of the `release` environment. After the first release, add the
   trusted publisher on the package's settings page and delete both the secret
   and the token.
4. **crates.io.** Sign in with GitHub. The first version, too, needs a token:
   store it as the secret `CARGO_REGISTRY_TOKEN` of the `release` environment.
   After the first release, add the trusted publisher on the crate's settings
   page and delete both the secret and the token; the workflow then uses
   trusted publishing on its own.

npm publishes with provenance, so each version on npm links to the commit and
the workflow run that built it.
