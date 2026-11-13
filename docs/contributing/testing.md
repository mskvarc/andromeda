# Testing

Every module has tests, serde helpers included. A test's name describes the scenario it tests.

## Unit tests

Unit tests sit beside the code they test in `#[cfg(test)]` modules. [Getting started](getting-started.md) gives the commands that run them without a live stack. A unit test never depends on timing, ordering or shared state that could make it flaky.

## Black-box test suite

`tools/andromeda-test-suite` tests the running broker over the network. Black-box tests live only there, in the tree that matches the behaviour they check.

| Tree | What it checks |
|---|---|
| `ngsi_ld` | Behaviour the NGSI-LD specification mandates, and the choices it leaves to the implementation |
| `andromeda` | Andromeda's extensions, limits and configurations |
| `etsi` | The ported ETSI conformance suite |
| `annex_c` | The examples of annex C of the specification |
| `distributed` | Distributed and federated operation |
| `pgbouncer` | Running behind PgBouncer |
| `interoperability` | The ported ETSI interoperability cases, run across several brokers |

Each tree runs against a Docker Compose stack. Run one with `tools/andromeda-test-suite/run-suite.sh <tree>`, which is what CI runs. The script also runs the `upgrade` tree, which holds the upgrade rehearsals described below rather than tests of the suite. [Test suite](../testing/test-suite.md) describes the stacks, how to run a tree by hand, and why two runs can't share a Docker host. The `pgbouncer` stack is very heavy, and needs a host with a lot of memory. It is very resource intensive. CI doesn't run it.

The suite isn't a default workspace member, so a plain `cargo clippy` skips it. CI lints it with the rest of the workspace, and you can lint it on its own the same way.

```sh
cargo clippy -p andromeda-test-suite --all-targets --all-features -- -D warnings
```

## Upgrade rehearsals

`tools/andromeda-upgrade-tests` builds databases at the schema an earlier release sealed, fills them with the rows that release stored, in the live branch and in Snapshot branches, and applies the migrations of the working tree, as the broker does when it opens a tenant. A migration that rewrites stored data, and a change to how the migrations reach the schemas of a tenant database, gets a rehearsal there. The rehearsals need a PostgreSQL server and no broker. CI runs them as the `upgrade` tree through `run-suite.sh`, and [Upgrade tests](../testing/upgrade-tests.md) describes how to run them on the host.

## The PostgreSQL image

The PostgreSQL image in `docker/images/postgresql` has check scripts of its own in its `tests/` directory. CI runs only the settings check, so run the others that cover your change by hand, and name them in the Verification section of the pull request. [PostgreSQL image checks](../testing/postgresql-image-checks.md) describes each script and how to run it.

## Writing suite tests

- Tests are typed. They use the `ngsi-ld` client builders, typed operations and typed response models, never raw JSON text, templated requests or JSONPath assertions.
- Every test runs fully in parallel in the default tenant, kept apart by its own ids and vocabulary. Use a private tenant only when nothing else can isolate the test, such as an exact listing of a whole tenant. Beyond the test groups some stacks need, which [Test suite](../testing/test-suite.md) lists, tests are never scheduled serially. An assertion over shared state checks only what the test created.
- A port of external material, meaning the ETSI suite or annex C, may assert more strictly than the original but never less. Where the original deviates from a standard, the port checks what it meant. A wrong annex C example is replayed corrected and marked with a registered correction id. Nothing specific to Andromeda goes into the `etsi` or `annex_c` trees.
- Some value shapes are settled. A plain Property value goes through JSON-LD compaction, so a one-element array reads back as its element, while a JsonProperty keeps its value as written. A value has time semantics only when its `valueType` is an NGSI-LD Date, Time or DateTime, and an undeclared string that looks like a time is a String. Tests assert both.

## What a change has to prove

- A behaviour fix adds a suite test that fails before the fix and passes after it. It goes in `ngsi_ld` for behaviour the specification defines or leaves to the implementation, in `andromeda` for an extension and in `distributed` for federation.
- The specification decides what a test expects. Cite the clause in the test when the expected value isn't obvious.
- Where the specification leaves a choice to the implementation, a test asserts the choice Andromeda made and cites the clause that leaves it open. Where the outcome is simply undefined, such as the order of notifications when no clause defines one, a test asserts nothing about it.
- When the broker disagrees with the specification, the test asserts the specification and fails until the fix lands. A deviation is never pinned as expected behaviour, even where the clause only says "should".
- A flaky test is a defect. Reproduce it, find the cause and fix the broker or the test's isolation. Never rerun at lower parallelism to get a pass.
- CI runs no suite tree on a pull request. Say in the Verification section of the pull request which trees you ran.
