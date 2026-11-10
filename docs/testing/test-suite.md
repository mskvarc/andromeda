# Test suite

`tools/andromeda-test-suite` holds the black-box tests of the Andromeda NGSI-LD context broker, written against the `ngsi-ld` client crate. Each test runs in a process of its own, beside every other test. It works in the shared default tenant unless it names a private one, and a namespace drawn per test keeps it apart from the rest. The `interoperability` and `pgbouncer` tests are the exceptions, and run one at a time.

The crate is a member of the root Cargo workspace but not one of its default members, so `cargo test`, `cargo check` and `cargo clippy` without `--workspace` skip it. It needs a running stack, so the CI unit-test steps leave it out. The CI clippy step does lint it with the rest of the workspace, so a suite test that doesn't compile fails `pr-ok`, even though no tree runs on a pull request. Its only Andromeda dependency is `andromeda-postgresql-seaquery`, and it reaches the broker only over the network.

The HTTP/3 tests talk to the broker through `reqwest`'s `http3` feature, which compiles only under `--cfg reqwest_unstable`. The root `.cargo/config.toml` sets that flag in `build.rustflags` for the whole workspace, because feature unification gives the suite's `reqwest` to every build that includes the suite, whether it starts from the repository root or from the runner. A `RUSTFLAGS` environment variable replaces those flags, so a build that sets it and includes the suite has to pass `--cfg reqwest_unstable` as well. A QUIC handshake that offers an application protocol other than `h3` needs a QUIC client of its own, and the suite builds that directly on `quinn`.

## Trees and stacks

| Test target | Tree | Stack |
|---|---|---|
| `etsi` | The ETSI NGSI-LD conformance suite, one test per ETSI case | `stacks/base` |
| `ngsi_ld_<area>` | Behaviour the specification mandates or leaves to the implementation | `stacks/base` |
| `annex_c` | The worked examples of annex C | `stacks/base` |
| `andromeda_<area>` | Andromeda's extensions, limits and configurations | `stacks/andromeda` |
| `distributed_<area>` | Distributed operations and federation across brokers | `stacks/multi` |
| `pgbouncer_<area>` | A broker serving every request through a transaction-pooling PgBouncer | `stacks/pgbouncer` |
| `interoperability` | The ETSI interoperability suites (`IOP_TP`), which run Create Entity, Retrieve Entity and Query Entities across registrations of every mode, each in a default-context and a user-context variant | `stacks/interoperability` |

The `ngsi_ld`, `andromeda`, `distributed` and `pgbouncer` trees build one test target per area (`ngsi_ld_entity`, `andromeda_snapshot`, `distributed_query` and so on), so a change to one area only compiles that area. `--test 'ngsi_ld_*'` selects a whole tree.

Every stack lives under `tools/andromeda-test-suite/stacks`. It runs the brokers, their databases, and a `runner` service that the tests run in. No service publishes a port on the host. The tests reach the brokers by their network names, and the brokers reach back to the notification receivers and context sources that each test serves from its own process. The runner reads its configuration from the stack's `runner.env` (`ANDROMEDA_TEST_SUITE__*`), which also raises `RUST_MIN_STACK` to 32 MiB for the longest request sequences of an unoptimised build.

Every broker in every stack runs over a PostgreSQL server of its own. Replicas aren't supported, so no two brokers share a server, a database or a tenant database, and no test writes through one broker to read through another. The services the stacks share are defined once in `stacks/templates/docker-compose.yaml`, and each stack extends them. They are a PostgreSQL server, a broker with the stacks' shared timings (a one-second expiry sweep and a two-second revalidation of hosted contexts), and the runner. Every stack still runs on its own.

`stacks/base` is one broker plus three Mosquitto brokers (anonymous, password and large-message), whose configurations live in `stacks/mosquitto`.

`stacks/multi` has six broker identities, `broker-a` to `broker-f`. A test registers brokers with one another, so each identity a topology names is a broker of its own, and a setting goes on an identity whose tests it doesn't affect. `broker-e` runs temporal Linked Entity retrieval and the language-ranges level of `q_target_languages`. `broker-f` runs the same level with forwarding bounds of 5 distinct entities, 15 source rows and 3 attribute instances.

### The andromeda stack

`stacks/andromeda` is a front broker on the broker's defaults with every extension but `q_target_languages` enabled and a fresh query walking by cursor, plus the brokers below. Settings that conflict neither with the front broker's nor with each other share a broker. A broker gets a service of its own only for a process-wide mode, or for a setting that would change what another broker's tests assert. Each compose service says which conflict it exists for.

| Service | Role | Settings |
|---|---|---|
| `broker` | `FRONT` | every extension but `q_target_languages` enabled, a fresh query walking by cursor, small delete-by-query and grouping bounds, 100,000 geometry positions, an 8 MiB page limit |
| `broker-conformance` | `CONFORMANCE` | conformance mode, with five extension flags still set to show that the mode overrides them |
| `broker-not-recorded` | `NOT_RECORDED` | Core writes not recorded, no advertised URL, forwarded fields believed only from a range the runner is never in, every other setting at its default |
| `broker-tls` | `TLS`, `TLS_MANAGEMENT`, `TLS_PLAINTEXT`, `TLS_MANAGEMENT_PLAINTEXT` | TLS and HTTP/3 on the API listener, TLS on the management listener, optional client certificates, with the identities in `stacks/andromeda/tls`; the two plaintext roles address its TLS ports without TLS |
| `broker-mutual` | `MUTUAL` | TLS with a TLS 1.3 floor and client certificates required, presenting `mutual.crt` from the `mutual-identity` volume, which a test overwrites with a broken certificate |
| `broker-rotating` | `ROTATING` | TLS presenting `rotating.crt` or `rotated.crt` from the `rotating-identity` volume, which a test swaps between the two |
| `broker-durable` | `DURABLE` | durable notification delivery, at most three Snapshots per tenant, a two-second wait for a preparing Snapshot |
| `broker-single-tenant` | `SINGLE_TENANT` | multi-tenancy disabled, a 4 KiB request body limit, durable delivery with a 5 KiB limit on HTTP notification bodies and four attempts within a second, no remote `@context` fetching |
| `broker-edge` | `EDGE`, `EDGE_HTTPS` | every forwarded field believed, cursor pagination available but not the default, the language-ranges level of `q_target_languages`, `federation.query.aggregation` set to `instances` |
| `broker-prefixed` | `PREFIXED` | an apiRoot under `/broker` on an origin nothing serves, cursor pagination disabled, the language-tags level of `q_target_languages`, tenant management enabled |
| `broker-observed` | `OBSERVED` | Prometheus metrics on the management port, at `METRICS__OBSERVED`; only the `observability/metrics` tests use it, so nothing else moves the series they read |
| `broker-waker` | `WAKER` | four tenants loaded at once, the default tenant among them, a five-second `min_wake_interval` and a one-second `woken_lifetime` |
| `broker-backlog` | `BACKLOG` | four tenants loaded at once, a one-second `min_wake_interval` and a five-second `woken_lifetime`, metrics served; only `a_obs_wbk_9001` uses it, since it reads the broker's metrics whole |
| `broker-roomy` | `ROOMY` | 41 tenants loaded at once, a five-second `min_wake_interval` and a five-second `woken_lifetime`, two connections per tenant, metrics served; only `a_obs_wbk_9002` uses it |
| `broker-overloaded` | `OVERLOADED` | the backlog broker's places and wake timings, metrics served; only `a_obs_wbk_9003` uses it, since it reads the broker's metrics whole |
| `broker-oversized` | `OVERSIZED` | metrics served, `request_limits.max_body` 1800 KiB, `notifications.delivery.max_mqtt_packet` 2 MiB and `notifications.delivery.max_http_body` 1900 KiB; only the three `a_obs_nsz` tests use it, one for each label of the oversized-notification counter (`mqtt-server-announced`, `max-mqtt-packet` and `max-http-body`), so each label moves only with what its own test sends |
| `broker-timed` | `TIMED` | a two-second `server.request_head_timeout` and a six-second `server.idle_timeout`; only the `transport/connection` tests use it, since an idle bound this short would close pooled connections under any other test just as it reuses them |
| `mosquitto-capped` | `CAPPED_MQTT_HOST` | an anonymous Mosquitto broker announcing a 1 MiB Maximum Packet Size, below `broker-oversized`'s `max_mqtt_packet`, configured by `stacks/mosquitto/capped.conf`; only the `a_obs_nsz` tests publish to it |
| `terminator` | `TERMINATOR`, `TERMINATOR_PLAINTEXT` | nginx terminating TLS 1.2 and 1.3 on port 8443 in front of `broker-edge`, setting `Forwarded` and `X-Forwarded-Proto` in place of whatever the client sent, and redirecting plaintext on port 8080 to its TLS listener; configured by `stacks/andromeda/nginx.conf` |

The TLS clients connect to `broker-tls` as `localhost`, the name its certificate carries (`CONNECT_TO__LOCALHOST`). `broker-mutual`, `broker-rotating` and `terminator` carry certificates for their service names, all issued by `tls/ca.crt`. `tls/stranger.crt` is a self-signed client certificate that no broker trusts.

A broker reloads its identity when the files it reads change, but the repository is mounted read-only. The identities that tests rewrite therefore live in the `mutual-identity` and `rotating-identity` volumes. The one-shot `identity-seed` service copies the stack's certificates into them on every `up`, the brokers mount them read-only, and the runner mounts them at the directories `runner.env` lists under `IDENTITY_DIRS`.

### The pgbouncer stack

`stacks/pgbouncer` is by far the heaviest stack. It runs fifteen brokers, each over a PostgreSQL server of its own, and the scale tests bring up a hundred tenant databases on a server. Each server is limited to 2 GB of memory, and the image sizes its buffers and work memory from that limit. Without the limit, each of the fifteen would size itself for the whole host, and together they would outgrow it. Even so, the stack needs a host with a lot of memory, and its tests run one at a time, because running more than one drives the host out of memory and the brokers die.

All but two of the brokers serve their requests through a PgBouncer in transaction mode in front of their PostgreSQL server, as `[database.pooler]` configures it. Provisioning, migration and maintenance still go to the server directly. Every broker owns its PostgreSQL server and, when it's pooled, its pooler, so a test that pauses, saturates or kills a pool, or counts a server's backends, only ever sees its own broker's. Every broker allows two minutes per request, and the servers under the scale tests accept 300 connections.

The poolers' configuration lives in `stacks/pgbouncer/pgbouncer`. Each pooler's file routes every database to its server under one shared login, so each tenant database gets a pool of its own on first use. The file then includes either `settings.ini` (8 server connections per database, 2 in reserve, servers kept 30 seconds when idle, 64 prepared statements) or `settings-tight.ini` (2, 1, 5 seconds and 32).

| Service | Role | Used by | Settings |
|---|---|---|---|
| `broker` | `FRONT` | `pgbouncer_tenancy`, `pgbouncer_semantics` | live delivery, 32 resident tenants |
| `broker-dormant` | `DORMANT` | `pgbouncer_dormant` | 32 resident tenants evicted after two idle minutes, durable delivery with ten attempts, residency sweep every 5 seconds, an evicted tenant woken for due work after 3 seconds |
| `broker-dormant-live` | `DORMANT_LIVE` | `pgbouncer_dormant` | live delivery with an evaluation queue of two changes and one lane, 3 resident tenants evicted after two idle minutes, residency sweep every 5 seconds |
| `broker-resilience` | `RESILIENCE` | `pgbouncer_resilience` | 128 resident tenants |
| `broker-pooled-durable` | `POOLED_DURABLE` | `pgbouncer_dormant`, `pgbouncer_semantics` | tight pooler, durable delivery, automatic maintenance and automatic repacks with the responsive profile, 128 resident tenants |
| `broker-direct` | `DIRECT` | `pgbouncer_dormant` | no pooler, 4 connections per tenant, 32 resident tenants |
| `broker-provisioning`, `broker-thrash`, `broker-zipf`, `broker-concurrency` | `PROVISIONING`, `THRASH`, `ZIPF`, `CONCURRENCY` | one `pgbouncer_scale` test each | 32 resident tenants |
| `broker-wide` | `WIDE` | `p_scl_bkd_0001` | tight pooler, 1,000 resident tenants; a resident tenant's background work returns to its database within 30 seconds, so only a server closed 5 seconds after its last transaction goes away between those rounds |
| `broker-crowded` | `CROWDED` | `p_scl_res_0003` | 1,000 resident tenants |
| `broker-burst-pooled`, `broker-burst-direct` | `BURST_POOLED`, `BURST_DIRECT` | `p_scl_bkd_0003` | tight pooler; no pooler |
| `broker-first-write` | `FIRST_WRITE` | `p_ten_prv_0002` | tight pooler, 2 connections per tenant, 32 resident tenants; a server the wake reconciliation pass opened on a new tenant's database is still idle when the tenant's runtime borrows one |

The `signaller` service stops and restarts `broker-dormant-live` for the two tests that stop a broker, `p_dor_ntf_0006` and `p_dor_ntf_0007`. It sends the broker SIGTERM and starts it again. For each request, busybox `nc` runs `stacks/pgbouncer/signaller/handle`, which drives the container runtime through the socket the service mounts. The runner reaches the signaller at `ANDROMEDA_TEST_SUITE__SIGNALLER`, and the test starts the broker again before it ends. `broker-dormant-live` is used only by `p_dor_ntf_0005`, `p_dor_ntf_0006` and `p_dor_ntf_0007`.

Every broker except the front one serves metrics and the tenant-management extension. Besides the brokers, `runner.env` names three things for each role. These are the broker's PostgreSQL server (`POOLING__SERVERS__<ROLE>`, reached with the server's administrative login), its pooler's admin console (`POOLING__POOLERS__<ROLE>`, database `pgbouncer`) and its metrics (`METRICS__<ROLE>`). It also sets the size of the scale tests, `POOLING__SCALE_TENANTS` tenants (three times the cache of 32) with load runs of `POOLING__LOAD_SECONDS`.

A test only looks at PostgreSQL and the pooler for what an operator would see there too. That means the console's pools and clients and its `PAUSE`, `RESUME`, `RECONNECT` and `KILL` of one database, sessions through the pooler that hold a database's pool, and the client backends in `pg_stat_activity`. Tests learn a tenant's database from `databaseName` in the tenant's usage, and its Entities from `entityCount`. Only one test touches Andromeda's own tables. `p_sem_mnt_0002` seeds a maintenance breach by reading `tenants` and writing to `postgresql_maintenance_targets` in the platform database. Every control command names one of the test's own tenant databases, so the resilience tests share nothing, and the scale tests each use brokers that no other test uses, with servers of their own under them. The nextest profiles give the `pgbouncer_*` binaries ten minutes per test, because a dormant test may wait 150 seconds for its tenant to be evicted and then wait out a retry interval that grew in the meantime.

### The interoperability stack

`stacks/interoperability` is five brokers, each over a database server of its own, plus the same `runner` service. The brokers reach one another, and the runner reaches each of them, by the service names `broker-1` to `broker-5`, so a registration `endpoint` and a hosted `@context` resolve the same way for both. The cases call the brokers b1 to b5, as the ETSI documentation does, and `stacks/interoperability/runner.env` maps them to the roles of the configuration.

| Case name | Service | Broker role | Peer role |
|---|---|---|---|
| b1 | `broker-1` | `FRONT` | `A` |
| b2 | `broker-2` | `B` | `B` |
| b3 | `broker-3` | `C` | `C` |
| b4 | `broker-4` | `D` | `D` |
| b5 | `broker-5` | `E` | `E` |

Registrations are global to a broker, so each case keeps to an entity type and entity identifiers in its own namespace, and every registration it creates names that type. A type of its own only isolates a case for inclusive and auxiliary registrations, though. An exclusive registration conflicts with every exclusive or redirect registration on the same broker that matches its entity id and any of its Attributes, whatever the entity type (ETSI GS CIM 009 v1.9.1 clauses 4.3.6.3 and 5.9.2.4). A redirect registration of `location` for every entity of one case's type matches the entity id of another case's exclusive registration of `location`. The Attribute names don't separate them either, since `location` is a core term and the default-context variant expands every other name under the same vocabulary. That's why the nextest profile `suite`, which `suite-verdict` inherits, puts the `interoperability` binary in a test group of one thread. The user-context variant stores a vocabulary on b1, and the other brokers resolve it by the URL b1 serves it at.

## Test groups

The nextest profiles `suite` and `suite-verdict` live in the root `.config/nextest.toml`. `suite-verdict` inherits `suite` and adds the JUnit report, so every group and override is written once. There are three groups.

- `interoperability` runs one test at a time, for the reason above.
- `pgbouncer` runs one test at a time too. Its stack is fifteen brokers over fifteen PostgreSQL servers, and one test at a time is what a host that holds the whole stack can still serve.
- `private-tenants` runs four tests at a time. It holds the `ngsi_ld`, `andromeda`, `distributed`, `etsi` and `annex_c` tests that work in a tenant of their own. Each of them makes the broker create a database and run the migrations in it, and the broker keeps 16 tenant runtimes resident. Started all at once, those provisionings queue on the database server for longer than the broker's admission deadline, and the broker answers 503 to the requests still waiting.

The `private-tenants` group lists its tests by id, in one override per tree. A new test that provisions a private tenant, through `private_tenant` or a client `with_tenant` that its namespace names, has to be added to its tree's override. `tools/andromeda-test-suite/check-private-tenants.sh` compares the lists with the tests. A test counts as private when its file declares a test and calls `broker::private_tenant(` or carries a `/// Tenant: private` line; a helper module that provisions tenants for other files' tests declares none and isn't listed. The script fails with `path:line` for a private test the group leaves out and for a listed test that isn't private. Run it after adding or moving a test, since it isn't part of the integration workflow.

The tests at one of the broker's size bounds run alone, with no other test beside them: the entity value and geometry position budgets, the request body limit, tens of thousands of instances or names, Linked Entity chains of a thousand Relationships and more, federated queries over thousands of Entities, and tens of tenants loaded at once, and the `ngsi_ld` tests of a 20 `MiB` notification, a large Entity's deletion, a federated query over thousands of Entities, a large mixed Snapshot or every page of a temporal capture, and the `distributed` tests that send 64 queries at once over the Entities or histories of four domains or capture thousands of Entities across brokers. Each keeps a broker and its database server busy for seconds of CPU per request, and on a GitHub-hosted runner of two CPUs a test running beside it holds its requests past the broker's 30-second request deadline. One override per tree lists them by id with `threads-required = "num-test-threads"`; a new test at a size bound is added to it. A private-tenant test among them stays in `private-tenants` too. The `ngsi_ld` tests that bound a broker's answer or its notifications by wall-clock time run alone for the same reason, in an override of their own.

A test that works in a tenant of its own brings it up on every broker it uses through `broker::private_tenant`, or `broker::provisioned_tenant` for a tenant named otherwise, never with a registration or a first write of its own. A broker bringing many tenants up at once answers the request that brings one up with 503, and only the helper sends it again until the delivery deadline.

The metrics test `a_obs_met_9001` gets ten minutes in both profiles, which share the override. It scrapes `broker-observed` again six minutes after its write, past the broker's five-minute idle timeout for gauges and counters. The wake backlog tests `a_obs_wbk_9001` to `a_obs_wbk_9003` and the clone tests `a_snp_cln_9170` and `a_snp_cln_9171` get five minutes. The wake backlog tests bring up forty tenants on a broker that keeps four loaded and then watch its wakes for up to a minute, the clone tests first build a Snapshot of 40,000 Entities, and on two CPUs each takes two minutes or more.

## Running

Run every command from the repository root. The stacks run `andromeda:local`, `andromeda-postgresql:local` and `andromeda-test-suite-runner:local`, and build each one from the working tree when it's missing. `--build` rebuilds the broker and database images before the stack starts, so the run tests the code on disk, uncommitted changes included.

```sh
docker compose -f tools/andromeda-test-suite/stacks/andromeda/docker-compose.yaml up -d --wait --build
```

The runner image is the `toolchain` stage of `stacks/runner/Dockerfile` and holds the pinned toolchain and cargo-nextest. Every stack's `runner` service uses the same image. CI builds the Dockerfile's `suite` stage instead, which adds the suite's test binaries, compiled without debug information, as the nextest archive `/andromeda-test-suite.tar.zst`. A run in that image compiles nothing. The service sits under the `runner` profile, so the first `run` builds it, or you can build it yourself with `docker compose -f tools/andromeda-test-suite/stacks/base/docker-compose.yaml --profile runner build runner`. The runner mounts the repository read-only at `/andromeda` and builds into the `test-suite-target` volume mounted at `/target` (its `CARGO_TARGET_DIR`), outside the checkout so a clone without a `target` directory needs no mountpoint in the read-only mount, while the `test-suite-cargo-registry` volume keeps dependencies between runs.

A stack runs a different broker, database or runner image, such as a release, when `ANDROMEDA_IMAGE`, `ANDROMEDA_POSTGRES_IMAGE` or `ANDROMEDA_TEST_SUITE_RUNNER_IMAGE` names it. Pull the image first. If a named image is missing, it gets built from the working tree under that name. `ANDROMEDA_PULL_POLICY` and `ANDROMEDA_POSTGRES_PULL_POLICY` set the Compose `pull_policy` of the broker and database services, `missing` unless set.

Bring a stack up and run its targets.

```sh
docker compose -f tools/andromeda-test-suite/stacks/base/docker-compose.yaml up -d --wait
docker compose -f tools/andromeda-test-suite/stacks/base/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test etsi
docker compose -f tools/andromeda-test-suite/stacks/base/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test 'ngsi_ld_*' --test annex_c
```

```sh
docker compose -f tools/andromeda-test-suite/stacks/andromeda/docker-compose.yaml up -d --wait
docker compose -f tools/andromeda-test-suite/stacks/andromeda/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test 'andromeda_*'
```

```sh
docker compose -f tools/andromeda-test-suite/stacks/multi/docker-compose.yaml up -d --wait
docker compose -f tools/andromeda-test-suite/stacks/multi/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test 'distributed_*'
```

The pgbouncer stack needs a host with a lot of memory to spare, as described above.

```sh
docker compose -f tools/andromeda-test-suite/stacks/pgbouncer/docker-compose.yaml up -d --wait
docker compose -f tools/andromeda-test-suite/stacks/pgbouncer/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test 'pgbouncer_*'
```

```sh
docker compose -f tools/andromeda-test-suite/stacks/interoperability/docker-compose.yaml up -d --wait
docker compose -f tools/andromeda-test-suite/stacks/interoperability/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --test interoperability
```

Nextest keeps its store under the workspace root whatever the target directory is, so every command passes the runner's `stacks/runner/nextest.toml`, which moves the store to `/target/nextest` in the `test-suite-target` volume. `--profile suite-verdict` writes a JUnit report to `/target/nextest/suite-verdict/junit.xml` there. A failing test prints every exchange it had with a broker.

`tools/andromeda-test-suite/run-suite.sh <tree>` does all of this for one tree, and it's what CI runs for every tree except `pgbouncer`, which CI never runs. It takes `ngsi_ld`, `andromeda`, `etsi`, `annex_c`, `distributed`, `pgbouncer` or `interoperability`. It rebuilds the runner, broker and database images from the working tree, except each one `ANDROMEDA_IMAGE`, `ANDROMEDA_POSTGRES_IMAGE` or `ANDROMEDA_TEST_SUITE_RUNNER_IMAGE` names, which it pulls instead. Then it brings the tree's stack up and runs the tree under `suite-verdict`, from the archive in a runner image it pulled, with `--archive-file /andromeda-test-suite.tar.zst --workspace-remap /andromeda` and a `binary_id` filterset in place of `--test`, and compiled from the working tree with a runner it built. Then it copies the report to `REPORT_DIR/<tree>.junit.xml` (`target/suite-reports` unless `REPORT_DIR` says otherwise) and takes the stack down with its databases, keeping the `test-suite-target` and `test-suite-cargo-registry` volumes. It exits with the tests' status, or with the status of the report copy when the tests passed, with 1 when a runner image it pulled has no archive, and with 2 when the tree is missing or unknown. To test a release, name its image in `ANDROMEDA_IMAGE`.

Two runs can't share a Docker host at the same time. Each stack has a fixed project name, so two runs of one stack would collide, and every stack uses the same two volumes, so runs of different stacks would too.

The support library's own tests send no request to a broker, but they need the runner's environment and the `broker` service on the network. The notification receivers and context sources they start find the address the brokers reach them at through a route to it. Run them with the base stack up.

```sh
docker compose -f tools/andromeda-test-suite/stacks/base/docker-compose.yaml run --rm runner cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite -p andromeda-test-suite --lib
```

Stop a stack with `docker compose -f tools/andromeda-test-suite/stacks/<stack>/docker-compose.yaml down`. Every stack names the same `test-suite-target` and `test-suite-cargo-registry` volumes, so `down -v` deletes the compiled suite and its dependencies along with the stack's databases. To drop the databases and keep the build, first remove the stack's services with their anonymous volumes using `docker compose -f tools/andromeda-test-suite/stacks/<stack>/docker-compose.yaml rm -s -f -v`, then run `down`.
