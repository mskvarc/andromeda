# Upgrade tests

`tools/andromeda-upgrade-tests` rehearses the upgrade of a tenant database that holds the data an earlier release stored. Each test creates a database of its own on a PostgreSQL server, builds it at the schema the earlier release sealed, writes the rows that release stored, applies the migrations this build embeds, and reads the result back.

The tests run the broker's own migration sets, so a rehearsal applies the migrations a deployed tenant applies, in the order it applies them. The schema is built by the tenant migrator cut at the last version v1.0.0 sealed, recorded in the tenant ledger. The upgrade is `migrate_tenant_db`, the call the broker makes when it opens a tenant whose schema is behind. The rows are written with sea-query statements, and a composite column of the old shape is filled from a JSON document with `jsonb_populate_record`, because no type of the broker binds that shape.

The tests never reach a broker over the network, and they don't need one. They need a PostgreSQL server with the `PostGIS`, `TimescaleDB` and `ltree` extensions and a login that may create databases and extensions.

## What is covered

The `seal_upgrade` target covers the conversion of data-integrity seals (see [Data-integrity seals](../data-integrity-seals.md)), which migrations 0032, 0033 and 0034 perform.

- A complete seal in current state becomes a typed seal of value kind 12, with its two seals in fields of their own and its proof typed.
- A seal naming a type and a value that is no reference is dropped with its own sub-attributes.
- A seal missing a proof member is dropped with its children, and the sibling behind it is kept with its parent renumbered.
- A top-level `ngsildproof` leaves current state and the search row, nets out of the census ledgers, and takes its history streams, rows and instance claims with it.
- A seal in a history row, a pending change snapshot, a notification event snapshot and an append intent entry is converted, and a top-level seal entry of an intent is dropped.
- A database that never stored a seal upgrades with nothing changed.

## Running

The crate is a member of the root Cargo workspace but not one of its default members, so `cargo test`, `cargo check` and `cargo clippy` without `--workspace` skip it. Its test targets fail without a server, so the default nextest profile in `.config/nextest.toml` leaves them out with a default filter, and every other profile inherits it. Every `cargo nextest run`, the CI unit-test step and the command in [Getting started](../contributing/getting-started.md) among them, runs the crate's own unit tests and its architecture check and skips the rehearsals, and a test target added later is skipped the same way. A filterset given with `-E` narrows that set and never widens it. The `upgrade` profile selects the crate instead. The CI clippy step lints the whole crate with the rest of the workspace.

The server is the image the stacks use, built from the working tree, which ships `TimescaleDB` in its TSL edition. Its superuser is `andromeda`, and `POSTGRES_PASSWORD` sets the password. The container publishes its port on the loopback interface only, on a port of its own, so it neither clashes with a PostgreSQL already running on the host nor accepts connections from the network.

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
