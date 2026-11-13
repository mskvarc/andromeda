# Upgrade tests

`tools/andromeda-upgrade-tests` rehearses the upgrade of a tenant database that holds the data an earlier release stored. Each test creates a database of its own on a PostgreSQL server, builds it at the schema the earlier release sealed, writes the rows that release stored, applies the migrations this build embeds, and reads the result back.

The tests run the broker's own migration sets, so a rehearsal applies the migrations a deployed tenant applies, in the order it applies them. `public` is built by the tenant migrator run up to the last version v1.0.0 sealed, recorded in the tenant ledger. A Snapshot's branch schema is built by the branch migrator run up to the same version inside the schema, with the schema first on the search path and recorded in the schema's own ledger, beside the Snapshot's control record and preparation row as v1.0.0 wrote them. The upgrade is `migrate_tenant_db`, the call the broker makes when it opens a tenant whose schema is behind, or, for the platform database, which is also the default tenant's, `migrate_platform_db`, the call the broker makes when it starts. The rows are written with sea-query statements, and a composite column of the old shape is filled from a JSON document with `jsonb_populate_record`, because no type of the broker binds that shape. A test writes the rows of a Snapshot branch through a pool whose connections put that branch's schema first on the search path, so the same statements write the live branch and every Snapshot branch.

The tests never reach a broker over the network, and they don't need one. They need a PostgreSQL server with the `PostGIS`, `TimescaleDB` and `ltree` extensions and a login that may create databases and extensions.

## What is covered

The crate's one test target, `upgrade`, has two groups of tests.

`seal` covers the conversion of data-integrity seals (see [Data-integrity seals](../data-integrity-seals.md)), which migrations 0032, 0033 and 0034 perform.

- A complete seal in current state becomes a typed seal of value kind 12, with its two seals in fields of their own and its proof typed.
- A seal naming a type and a value that is no reference is dropped with its own sub-attributes.
- A seal missing a proof member is dropped with its children, and the sibling behind it is kept with its parent renumbered.
- A top-level `ngsildproof` leaves current state and the search row, nets out of the census ledgers, and takes its history streams, rows and instance claims with it.
- A seal in a history row, a pending change snapshot, a notification event snapshot and an append intent entry is converted, and a top-level seal entry of an intent is dropped.
- A database that never stored a seal upgrades with nothing changed.

`lockstep` covers how every schema of a tenant database moves through the migrations together, as [Schema](../database/schema.md#migrations) describes.

- A published and a staging Snapshot convert their seals as the live branch does. The seal of an attribute becomes typed, a top-level seal leaves the current state, the search row and the census, and every schema's ledger ends at the last migration.
- `snapshot_preparation` keeps no branch version and no upgrade failure of its own.
- An upgrade cut short after `public` applied the seal conversion resumes with the Snapshot branch, which converts its seals before the types change.
- A Snapshot branch whose seal conversion fails stops the upgrade with an error naming the Snapshot, its branch and the migration. `public` and the Snapshot branch migrated before it stay at the conversion, and the failing branch keeps its seal as v1.0.0 stored it. Once the branch can take the conversion, the next upgrade brings every schema through the last migration and converts the seals the failing branch kept.
- A Snapshot branch whose ledger cannot be read stops the upgrade before any migration runs, with an error naming the branch.
- A branch schema no control record names, which the expiry sweep drops, is never migrated, so it keeps no tenant from opening even when it could not take a migration.
- A Snapshot created through the broker's own creation path after the upgrade builds its branch through every branch migration, against the types the later tenant migrations changed.
- A database no migration has reached takes the whole tenant set.
- A database whose ledger records an applied branch migration other than the one this build embeds is refused, even when nothing is left to apply.
- The platform database runs the platform set and then converts the seals of its live branch and its Snapshot branch, with every ledger at the last migration of its set.

## Running

The crate is a member of the root Cargo workspace but not one of its default members, so `cargo test`, `cargo check` and `cargo clippy` without `--workspace` skip it. Its test target fails without a server, so the default nextest profile in `.config/nextest.toml` leaves it out with a default filter, and every other profile inherits it. Every `cargo nextest run`, the CI unit-test step and the command in [Getting started](../contributing/getting-started.md) among them, runs the crate's own unit tests and its architecture check and skips the rehearsals, and a test target added later is skipped the same way. A filterset given with `-E` narrows that set and never widens it. The `upgrade` profile selects the crate instead, and `upgrade-verdict` adds a JUnit report to it. The CI clippy step lints the whole crate with the rest of the workspace.

CI runs the rehearsals as the `upgrade` suite tree, as [Test suite](test-suite.md) describes, on every push to `main` and every run of `ci` started by hand. A pull request skips it, as it skips every tree. The tree runs the published database image and the runner, whose nextest archive carries the rehearsals beside the suite, over the stack `tools/andromeda-test-suite/stacks/upgrade`, which is one PostgreSQL server and the runner. `tools/andromeda-test-suite/run-suite.sh upgrade` runs the same tree locally.

To run the rehearsals on the host, start a server from the image the stacks use, built from the working tree, which ships `TimescaleDB` in its TSL edition. Its superuser is `andromeda`, and `POSTGRES_PASSWORD` sets the password. The container publishes its port on the loopback interface only, on a port of its own, so it neither clashes with a PostgreSQL already running on the host nor accepts connections from the network.

```sh
docker build -t andromeda-postgresql:upgrade-tests docker/images/postgresql
docker run -d --name upgrade-tests-postgres -e POSTGRES_PASSWORD=upgrade -p 127.0.0.1:55432:5432 andromeda-postgresql:upgrade-tests
```

`ANDROMEDA_UPGRADE_TESTS__SERVER` names the server. It's a PostgreSQL URL with an administrative login, naming any database the login may connect to. The tests derive the URL of each database they create from it. Run the target from the repository root.

```sh
ANDROMEDA_UPGRADE_TESTS__SERVER=postgres://andromeda:upgrade@127.0.0.1:55432/andromeda cargo nextest run --profile upgrade -p andromeda-upgrade-tests
```

Without `--profile upgrade`, the run selects none of the rehearsals.

Every test creates a database named `upgrade_` followed by a random identifier, so the tests run in parallel and share nothing. A test that passes drops its database. A failing test leaves its database behind, so you can inspect what the upgrade left in it, and a later run never reuses it.

`cargo nextest run -p andromeda-upgrade-tests` runs the crate's own unit tests, which need no server.
