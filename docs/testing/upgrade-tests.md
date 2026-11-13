# Upgrade tests

`tools/andromeda-upgrade-tests` rehearses the upgrade of a tenant database that holds the data an earlier release stored. Each test creates a database of its own on a PostgreSQL server, builds it at the schema the earlier release sealed, writes the rows that release stored, applies the migrations this build embeds, and reads the result back.

The tests run the broker's own migration sets, so a rehearsal applies the migrations a deployed tenant applies, in the order it applies them. `public` is built by the tenant migrator run up to the last version v1.0.0 sealed, recorded in the tenant ledger. A Snapshot's branch schema is built by the branch migrator run up to the same version inside the schema, with the schema first on the search path and recorded in the schema's own ledger, beside the Snapshot's control record and preparation row as v1.0.0 wrote them. The upgrade is `migrate_tenant_db`, the call the broker makes when it opens a tenant whose schema is behind, or, for the platform database, which is also the default tenant's, `migrate_platform_db`, the call the broker makes when it starts. The rows are written with sea-query statements, and a composite column of the old shape is filled from a JSON document with `jsonb_populate_record`, because no type of the broker binds that shape. A test writes the rows of a Snapshot branch through a pool whose connections put that branch's schema first on the search path, so the same statements write the live branch and every Snapshot branch.

The tests never reach a broker over the network, and they don't need one. They need a PostgreSQL server with the `PostGIS`, `TimescaleDB` and `ltree` extensions and a login that may create databases and extensions.

## What is covered

The crate's one test target, `upgrade`, has two groups of tests.

`seal` covers the conversion of data-integrity seals (see [Data-integrity seals](../data-integrity-seals.md)), which migrations 0032, 0033 and 0034 perform.

- A complete seal in current state becomes a typed seal of value kind 12, with its two seals in fields of their own and its proof typed. The broker's own decoder reads it back with every member as written.
- A seal naming a type no Entity carries adds it to the type dictionary, and a type named twice is sealed once, at its first position.
- A seal naming a type and a value that is no reference is dropped with its own sub-attributes.
- A seal missing a proof member is dropped with its children, and the sibling behind it is kept with its parent renumbered.
- A seal no write admits is dropped beside a complete one that is kept. The cases are a `created` that clause 4.6.3 doesn't spell (a seventh fractional digit, an offset, lowercase designators, a day its month lacks, hour 24, second 60, no seconds, no datatype), an empty `cryptosuite`, a value of two proofs, a missing or empty subproperty, the NGSI-LD Null as the entity or a type, an entity that is no URI, is named by reference or is 1024 bytes long, and a seal that is a Relationship.
- Seals at the edges of what the broker accepts are kept and read back through the broker's own decoder: a URL port of 65535, leading zeros included, a `created` on the leap day of year 0000, a `created` with a comma before its fraction, an IRI made of the first and last characters of the first three `ucschar` ranges, and a URL with no path one byte below the identifier bound. A port of 65536, a noncharacter between two `ucschar` ranges and a URL at the identifier bound drop their seal.
- The conversion tests a host name alike on a server that reads a backslash in a plain string constant as an escape, where `standard_conforming_strings` is off. A seal naming a host of two labels joined by an underscore is dropped there too, and one naming an ordinary host is kept.
- Every URI the conversion admits is one the broker reads. A corpus of some 46,000 candidates crosses the hosts, ports, paths, queries and fragments of a URL and the opaque paths of other schemes, and each one the conversion's own test admits is parsed by the broker, is at most one byte longer in the spelling the broker stores, and is an identifier when it is within the bound.
- A top-level `ngsildproof` leaves current state and the search row, nets out of the census ledgers, and takes its history streams, rows and instance claims with it. The instances behind it in current state close up their ordinals, as a write removing it would.
- The streams of a top-level `ngsildproof` are counted out of the stream census under every type the Entity's identity row records for its Evolution, the types the broker counts a stream under, and no type membership row is read for them.
- A seal in a history row, a pending change snapshot, a notification event snapshot and an append intent entry is converted, and a top-level seal entry of an intent is dropped.
- A top-level seal held only in the previous state of a pending change or a notification event is dropped, and the instances behind it close up their ordinals.
- A top-level seal a pending change or a notification event touched leaves the change's three aligned arrays, its transition, attribute name and `datasetId` together, even when neither state holds it any longer.
- In a database that names `ngsildproof`, a pending change, a notification event and an append intent that hold no seal keep the row versions v1.0.0 wrote, so the upgrade rewrites no queued row it does not convert.
- Seals in history the columnstore holds are converted, and a top-level seal's stream leaves it, while the session lets one statement decompress a single row. The columnstore is turned on with the layout the broker gives it and every chunk is converted, as the broker does at runtime, since no migration does.
- The check that a tombstone of history holds nothing beside its deletion is added again without `proof_seals` and without reading a row of history, so the hypertable and the chunks v1.0.0 wrote hold it unvalidated. It refuses a tombstone holding a member all the same, in rowstore history and in history the columnstore holds.
- The upgrade leaves no `seal_conversion_` function in `public`. Migration 0034 drops them once every branch is converted, and the two that convert the queues are temporary and never reach the schema.
- A database that never stored a seal upgrades with nothing changed.

`lockstep` covers how every schema of a tenant database moves through the migrations together, as [Schema](../database/schema.md#migrations) describes.

- A published and a staging Snapshot convert their seals as the live branch does. The seal of an attribute becomes typed, a top-level seal leaves the current state, the search row and the census, and every schema's ledger ends at the last migration.
- `snapshot_preparation` keeps no branch version and no upgrade failure of its own.
- An upgrade cut short after `public` applied the seal conversion resumes with the Snapshot branch, which converts its seals before the types change.
- A Snapshot branch whose seal conversion fails stops the upgrade with an error naming the Snapshot, its branch and the migration. `public` and the Snapshot branch migrated before it stay at the conversion, and the failing branch keeps its seal as v1.0.0 stored it. Once the branch can take the conversion, the next upgrade brings every schema through the last migration and converts the seals the failing branch kept.
- A Snapshot branch whose ledger cannot be read stops the upgrade before any migration runs, with an error naming the branch.
- A branch schema no control record names, which the expiry sweep drops, is never migrated, so it keeps no tenant from opening even when it could not take a migration.
- A Snapshot created through the broker's own creation path after the upgrade builds its branch through every branch migration, against the types the later tenant migrations changed and without the conversion functions the upgrade dropped.
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
