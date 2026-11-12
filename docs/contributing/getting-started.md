# Getting started

## Toolchain

`rust-toolchain.toml` pins the nightly toolchain the workspace builds with, together with `clippy` and `rustfmt`. Rustup installs it the first time you run `cargo` in the repository. The workspace uses Rust edition 2024.

The SQL tooling calls two external programs, `pg_format` and Squawk. On macOS, install `pg_format` with Homebrew. Homebrew has no Squawk formula, so install Squawk through npm.

```sh
brew install pgformatter node
npm install -g squawk-cli@2.70.0
```

CI runs pg_format 5.11 and Squawk 2.70.0. Another pgFormatter release can lay the migrations out differently, and `andromeda-sql` refuses any Squawk release other than 2.70.0. Building `andromeda-sql` also compiles the PostgreSQL parser, which needs a C compiler and `libclang`. On macOS, the Xcode Command Line Tools provide both. [SQL tooling](../database/sql-tooling.md) covers installing the tools on Linux.

CI also runs the tests with cargo-nextest, and checks TOML formatting with taplo, unused dependencies with cargo-machete, advisories and licences with cargo-deny, and shell scripts with shellcheck. Install cargo-nextest, taplo, cargo-deny and shellcheck with Homebrew. Homebrew has no cargo-machete formula, so install it with Cargo.

```sh
brew install cargo-nextest taplo cargo-deny shellcheck
cargo install --locked cargo-machete@0.9.2
```

CI runs cargo-nextest 0.9.149, taplo 0.10.0, cargo-deny 0.20.2, shellcheck 0.11.0 and cargo-machete 0.9.2. The workspace refuses a cargo-nextest older than 0.9.145.

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
cargo fmt --all
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

`cargo fmt` runs the toolchain `rust-toolchain.toml` pins, which is the one CI checks formatting with. Don't override it with `+nightly`, because a newer nightly can format differently.

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
