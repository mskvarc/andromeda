# Getting started

## Toolchain

`rust-toolchain.toml` pins the stable toolchain the workspace builds, lints and tests with, together with `clippy`. Rustup installs it the first time you run `cargo` in the repository. The workspace uses Rust edition 2024.

The broker's and the suite runner's Dockerfiles build from the official `rust` image whose tag names the same version, and the runner takes its toolchain from that image. The `toolchain_pin` architecture gate fails while a tag names another version. Dependabot moves `rust-toolchain.toml` and both tags in one pull request, the `rust-toolchain` group of `.github/dependabot.yaml`.

Formatting runs a pinned nightly rustfmt, because `rustfmt.toml` uses options only a nightly reads. Install it once.

```sh
rustup toolchain install nightly-2026-09-25 --profile minimal --component rustfmt
```

The SQL tooling calls two external programs, `pg_format` and Squawk. On macOS, install `pg_format` with Homebrew. Homebrew has no Squawk formula, so install Squawk through npm.

```sh
brew install pgformatter node
npm install -g squawk-cli@2.73.0
```

CI runs pg_format 5.11 and Squawk 2.73.0. Another pgFormatter release can lay the migrations out differently, and `andromeda-sql` refuses any Squawk release other than 2.73.0. Building `andromeda-sql` also compiles the PostgreSQL parser, which needs a C compiler and `libclang`. On macOS, the Xcode Command Line Tools provide both. [SQL tooling](../database/sql-tooling.md) covers installing the tools on Linux.

CI also runs the tests with cargo-nextest, and checks TOML formatting with taplo, unused dependencies with cargo-machete, advisories and licences with cargo-deny, and shell scripts with shellcheck. Install cargo-nextest, taplo, cargo-deny and shellcheck with Homebrew. Homebrew has no cargo-machete formula, so install it with Cargo.

```sh
brew install cargo-nextest taplo cargo-deny shellcheck
cargo install --locked cargo-machete@0.10.1
```

CI runs cargo-nextest 0.9.152, taplo 0.10.0, cargo-deny 0.22.0, shellcheck 0.11.0 and cargo-machete 0.10.1. The workspace refuses a cargo-nextest older than 0.9.145.

The black-box test suite and the local stacks need Docker with Compose.

## Workspace layout

- `src/` is the broker binary. It has no library target.
- `crates/` holds every library crate, each with one responsibility. [Architecture](architecture.md) describes how they fit together.
- `tools/andromeda-test-suite` holds the black-box tests, which [Testing](testing.md) describes.
- `tools/andromeda-sql` formats, lints and seals the SQL migrations, as [Database migrations](database-migrations.md) describes.
- `migrations/` holds the schema.

## Before a pull request

CI requires every one of these checks to pass, so run them before you open a pull request.

```sh
cargo +nightly-2026-09-25 fmt --all
taplo fmt
cargo clippy --workspace --all-targets --all-features
cargo nextest run --workspace --exclude andromeda-test-suite
cargo machete --with-metadata
cargo deny --all-features --locked check
git ls-files -z '*.sh' | xargs -0 shellcheck
cargo run -p andromeda-sql -- format --check
cargo run -p andromeda-sql -- lint
cargo run -p andromeda-sql -- verify --against origin/main
```

Format with exactly that nightly, the one CI checks formatting with. A plain `cargo fmt` runs the stable rustfmt, which ignores the unstable options and rewrites the imports, and a floating `+nightly` can format differently.

The `andromeda` crate is a binary with no library target, so never pass it `--lib`. The black-box suite needs a live stack, so it is left out here. [Testing](testing.md) explains how to run the suite.

The workspace is large and linking it takes a while. When you only want to know that the code compiles, `cargo check` or `cargo clippy` is much faster than `cargo build`, which is only worth running when you're going to start the binary.

## Configuration changes

If you change a configuration key, regenerate the configuration template, JSON Schema and descriptor, then commit the files it rewrites.

```sh
ANDROMEDA_REGENERATE=1 cargo test -p andromeda-configuration
```

## Grammar changes

The grammar derive of `andromeda-ngsi-ld-lexicon` reads one file per type, so each grammar under its `grammar/` directory is also checked in under `grammar/.generated/`, followed by the shared Unicode rules of `_unicode.abnf`. The crate's tests fail when a composed copy no longer matches. If you change a grammar or the shared rules, rewrite the composed copies and commit them.

```sh
ANDROMEDA_REGENERATE=1 cargo test -p andromeda-ngsi-ld-lexicon
```

A new grammar needs its composed copy before a type can derive from it, because the crate doesn't compile while a derive names a file that doesn't exist. Add the grammar, run the command above, and then point the derive at the composed copy.
