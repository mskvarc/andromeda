<!--
Security vulnerabilities are reported privately, as SECURITY.md describes, not fixed in a pull request.
-->

Closes #

## Change

<!-- What the broker does differently afterwards. -->

## Cause

<!-- Why the change is needed. When the specification decides the behaviour, cite the ETSI GS CIM 009 v1.9.1 clause. -->

## Checklist

- [ ] `cargo +nightly fmt` leaves no changes.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --workspace --exclude andromeda --exclude andromeda-test-suite --all-features` and `cargo test -p andromeda --bins` pass.
- [ ] `tools/andromeda-test-suite/run-suite.sh <tree>` passes for every tree the change touches.
- [ ] Configuration changed: `ANDROMEDA_REGENERATE=1 cargo test -p andromeda-configuration` was run.
- [ ] Migration changed: `cargo run -p andromeda-sql -- format --check`, `lint` and `verify` pass.
