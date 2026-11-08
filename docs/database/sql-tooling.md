# SQL tooling

The migrations in `migrations/` are the schema contract of every release, and `andromeda-sql` keeps them in shape. It is a package of the Cargo workspace, kept in `tools/andromeda-sql`, but it isn't one of the default members. A plain `cargo check` at the root skips it, while `--workspace` or `-p andromeda-sql` builds it.

## Commands

Run the tool through Cargo from anywhere in the repository.

```sh
cargo run -p andromeda-sql -- format
cargo run -p andromeda-sql -- format --check
cargo run -p andromeda-sql -- lint
cargo run -p andromeda-sql -- seal v1.2.0
cargo run -p andromeda-sql -- verify
cargo run -p andromeda-sql -- verify --against origin/main
cargo run -p andromeda-sql -- verify --release v1.2.0 --against v1.1.0
```

`format` rewrites every migration that isn't in the canonical layout and prints the path of each file it changed. It lays out every file before it writes any, so if one can't be laid out, none of them is touched. Sealed migrations are skipped, so a later change to the layout never rewrites a file that has shipped.

`format --check` writes nothing. It lists the migrations that aren't in the canonical layout and exits non-zero if there are any. Sealed migrations aren't checked. CI runs this.

`lint` runs Squawk over every migration that isn't sealed yet, then Andromeda's own rules over the schema all the migrations declare. Each finding is printed as a diagnostic with the rule name, the message, the file, line and column, the surrounding lines with the offending text underlined, and the suggested fix. A Squawk finding also links the rule's page. A broken rule is a warning. A migration Squawk can't parse is an error, and Andromeda's rules then don't run at all, because they couldn't read the schema either. A `formatter-vocabulary` finding can point at `.pg_format_functions` or `.pg_format_keywords` instead of a migration. When there are findings, the run ends with a count per severity and exits non-zero. A clean run prints nothing. Sealed migrations still count as part of the schema, but nothing is reported in them. CI runs this.

`seal <version>` records every unsealed migration in `migrations/seal-ledger.toml` under the stable release `version` (`vX.Y.Z`), with the SHA-256 of its bytes. It writes nothing unless `verify`, `format --check` and `lint` pass, the version is higher than the last sealed release, and there is something to seal. Pre-releases are never sealed. [Migration sealing](migration-sealing.md) explains the contract and the ledger.

`verify` fails when a sealed migration was edited, deleted or renamed, when a new migration has a version at or below a sealed one, or when the ledger isn't in the exact form the tool writes. It names the file and the broken rule. With `--against <revision>`, it also fails if any ledger section at that git revision has changed since. With `--release <tag>`, it also checks that the tree can be tagged as that release. A stable `vX.Y.Z` needs its own section as the last one and every migration sealed, and a pre-release tag needs nothing more. The `sql` job of the `ci` workflow runs the first two on every pull request that isn't a draft, on every push to `main`, and when the workflow is started by hand. The release workflow runs `--release` before it builds anything, as [Release process](../release-process.md) describes.

`--root <PATH>` points the tool at another checkout. Without it, the tool works on the repository it was built from.

## How the migrations are laid out

`migrations/` holds the three set directories `platform`, `tenant` and `branch`, the seal ledger `seal-ledger.toml`, and nothing else. The ledger appears with the first sealed release, and until then the tool reads a missing ledger as an empty one. A set directory holds only migration files named `NNNN_snake_case.sql`, which means four version digits, an underscore, and words of lowercase letters and digits joined by single underscores, the first word starting with a letter. The three sets share one version space. Each version belongs to exactly one file across all three, and a new migration in any set takes the next free number. If a file breaks either rule, the tool refuses to run and names the file. It ignores names that start with a dot.

## Canonical layout

The canonical layout is whatever `pg_format` produces with the options in `.pg_format` at the repository root. Two more files there teach it words its own dictionary lacks. `.pg_format_keywords` lists keywords, and `.pg_format_functions` lists functions, among them TimescaleDB and PostGIS functions and Andromeda's own trigger functions. No `pg_format` option covers three further adjustments, so the tool makes them itself.

- Function statements stay exactly as written. `pg_format` reformats a `plpgsql` body as if it were plain SQL and unwraps a body written as a string literal. So before it runs, the tool swaps each `CREATE FUNCTION` and `CREATE PROCEDURE` statement for a placeholder, and puts the original back afterwards. The PostgreSQL parser decides where each statement starts and ends, from its first token to its closing semicolon, without the comments above it. If the placeholders come back in a different number or order, the tool stops and names the file.
- A parenthesised list that fits on one line and holds no parentheses of its own goes back onto the line that opens it, so `pg_format`'s `identity (\n    cache 1)` becomes `identity (cache 1)`. Lists that span several lines stay as `pg_format` left them, and so does every list inside a function statement.
- The blank lines `pg_format` leaves after the last statement are removed, so every migration ends with exactly one newline.

## Lint rules

Squawk takes its configuration from `.squawk.toml` at the repository root, which also says why each rule is turned off or on. The tool hands Squawk the PostgreSQL version itself, using the oldest release the broker supports, so a rule that depends on the server version judges a migration against the oldest server it may meet. Every migration is linted as if it runs in a transaction, because the migrator wraps each file in one.

Four default rules are off because they don't match how the migrations run. They are `prefer-robust-stmts`, `require-concurrent-index-creation`, `require-statement-timeout` and `require-lock-timeout`. Two more, `prefer-bigint-over-int` and `prefer-bigint-over-smallint`, are off because Andromeda's `integer-width` rule decides integer width instead. `adding-foreign-key-constraint` and `constraint-missing-not-valid` are off because they can't tell a table created earlier in the same unreleased migrations from a live one without an inline suppression, and no migration may carry one. Four opt-in rules are on, because a migration has no business changing server, database or role settings, or disabling a trigger. They are `ban-alter-system-options`, `ban-alter-database-options`, `ban-alter-role-options` and `ban-disable-trigger`.

Andromeda's own rules cover the conventions Squawk can't express, and they run after it. They read the schema from PostgreSQL's syntax tree of every migration, along with the two `pg_format` vocabulary files and the seal ledger. [SQL lint rules](sql-lint-rules.md) describes each one and why it exists. No migration can turn a rule off, and any exception a rule makes is data in that rule's own module. `formatter-vocabulary` asks `pg_format` which functions it knows, so `lint` needs `pg_format` installed as well as Squawk.

## Installing pg_format

The tool needs `pg_format` from pgFormatter. CI pins pgFormatter 5.11, and the committed migrations are laid out the way that release lays them out, so install the same version locally. On macOS, Homebrew has it.

```sh
brew install pgformatter
```

On any other system with Perl, build it from source.

```sh
curl -fsSL https://github.com/darold/pgFormatter/archive/refs/tags/v5.11.tar.gz | tar -xz
cd pgFormatter-5.11
perl Makefile.PL
make
sudo make install
```

The tool uses the first `pg_format` on `PATH`. To pick a different one, set `PG_FORMAT` to its path or to a name on `PATH`. Before it reads any migration, the tool formats one statement with the repository options as a test, so a release too old for them is refused right away.

## Installing Squawk

The tool accepts Squawk 2.70.0 and no other release, because a rule added or changed in another release could pass or fail the migrations differently from CI. Homebrew has no Squawk formula, so on macOS install it with npm, then check that `squawk --version` prints `squawk 2.70.0`.

```sh
npm install -g squawk-cli@2.70.0
```

On Linux, download the `squawk-linux-x64` or `squawk-linux-arm64` binary from the Squawk 2.70.0 release on GitHub and put it on `PATH`.

The tool uses the first `squawk` on `PATH`. To pick a different one, set `SQUAWK` to its path or to a name on `PATH`.

Building the package also compiles the PostgreSQL parser through the `pg_query` crate, which needs a C compiler and `libclang`.

## Tests

`cargo test -p andromeda-sql` runs the package's tests. Some of them call `pg_format` and Squawk, so the workspace unit-test step leaves the package out. The `sql` job of the `ci` workflow installs both tools and runs these tests instead, on every pull request that isn't a draft, on every push to `main`, and when the workflow is started by hand.

Two of the tests check that every committed migration is already in the canonical layout and passes the lint. The `seal` test target seals fixture repositories. It checks that every way of changing a sealed file or its section makes `verify` fail, and which trees `verify --release` lets through for a stable and for a pre-release tag. For `--against` it commits the fixture to a git repository of its own, so it needs `git` on `PATH`. The rules have unit tests over small migrations written inline, which check what a rule accepts, what it refuses, and the line and column of each finding.

The `schema` test target holds the schema's design pins. It checks the shape of individual relations on the same declared schema the rules read, such as the per-axis stream indexes of attribute history, the grain of `current_attributes`, value placement and its triggers, the append-only triggers of the dictionaries and the storage parameters of the hot tables. It also ties the byte limits the migrations declare to the Rust constants the broker enforces them with. A lint never reads Rust, so the test target takes those constants from the crates that define them, as development dependencies.
