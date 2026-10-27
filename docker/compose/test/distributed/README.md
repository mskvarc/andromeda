# Distributed Andromeda topology

Real multi-instance Andromeda for validating NGSI-LD distributed operations (Context Source Registration, forwarding, federation, entity maps) end-to-end over HTTP. This is the system-level counterpart to the in-process `distop_*` suites, which exercise forwarding against in-process mock Context Sources.

See [`docs/distributed-operations-test-plan.md`](../../docs/distributed-operations-test-plan.md) for how the suite uses this topology and [`docs/distributed-operations-conformance-register.md`](../../docs/distributed-operations-conformance-register.md) for the requirement register the suite implements.

## Topology

Three broker identities, each backed by its own TimescaleDB/PostGIS Postgres server (`postgres-a`, `postgres-b`, `postgres-c`, one database each, created by `initdb/<role>/10-create-broker-database.sql`). The stores are fully isolated: nothing broker-b persists is reachable from broker-a's server, so any state that shows up on a peer got there over NGSI-LD, never through a shared table. Each identity has a distinct `ANDROMEDA__FEDERATION__IDENTITY__SOURCE_ID` / `ANDROMEDA__FEDERATION__IDENTITY__SOURCE_ALIAS`, which is what makes `Via` / `contextSourceAlias` loop detection meaningful across instances.

broker-a additionally runs as a **replica pair**: `broker-a2` is a second instance of the same identity (same source id, alias and database on `postgres-a`). It exists to prove that durable state - EntityMaps, registrations, platform routing - is shared across the instances of one broker and only across those: a map minted on broker-a is `200` on broker-a2 and `404` on broker-b.

| Instance | Host URL (test client) | Peer URL (in CSR `endpoint`) | Alias | Store |
|---|---|---|---|---|
| broker-a | `http://127.0.0.1:1061/ngsi-ld/v1/` | `http://broker-a:1056` | `broker-a` | `postgres-a` (host `5442`) |
| broker-a2 | `http://127.0.0.1:1064/ngsi-ld/v1/` | `http://broker-a2:1056` | `broker-a` | `postgres-a` (host `5442`) |
| broker-b | `http://127.0.0.1:1062/ngsi-ld/v1/` | `http://broker-b:1056` | `broker-b` | `postgres-b` (host `5443`) |
| broker-c | `http://127.0.0.1:1063/ngsi-ld/v1/` | `http://broker-c:1056` | `broker-c` | `postgres-c` (host `5444`) |

The two URL families matter: the test client on the host reaches brokers via published `localhost` ports, but a registration's `endpoint` must be the compose-network name, because one broker calls another over that network - never via `localhost`.

## Connection budget

Each Postgres server runs with `max_connections = 400`. The brokers run with `ANDROMEDA__DATABASE__MAX_CONNECTIONS=16` and `ANDROMEDA__TENANCY__DATABASES__MAX_CONNECTIONS=8`, and idle connections are reclaimed after 30 s. The busiest server (`postgres-a`, broker-a and its replica broker-a2) holds 2 x (16 + tenants x 8) for the tenants live at once: the default tenant, the four tenants the harness leases to concurrent topologies (one per test thread), and the tenants the tenant-specific suites name, plus the harness's own lazily opened pools. With a dozen tenants live that approaches 250, which the image default of 100 cannot hold.

## Bring up / tear down

```sh
# build the broker image once and start the stack
docker compose -f docker/compose/test/distributed/docker-compose.yaml up --build -d

# wait until all four brokers report healthy
docker compose -f docker/compose/test/distributed/docker-compose.yaml ps

# logs for one instance
docker compose -f docker/compose/test/distributed/docker-compose.yaml logs -f broker-a

# stop and remove, including the per-server data volumes
docker compose -f docker/compose/test/distributed/docker-compose.yaml down -v
```

The broker image (`docker/images/andromeda/Dockerfile`) builds a release binary and ships no config file - every setting comes from `ANDROMEDA_*` env vars, which all have defaults. The Postgres image (`docker/images/postgresql/Dockerfile`) provides TimescaleDB + PostGIS, required by the tenant migrations.

## Notes

- **Fault injection** (timeouts, `403`, malformed payloads, loops, unsupported operations) is not driven from these real peers - a real Andromeda peer answers correctly by construction. Those cases use the in-process `ContextSourceServer` mock as the registered peer, per the plan. This stack is for genuine broker-to-broker federation semantics; the mock is for controllable adversarial peers.
- **Mock reachability**: brokers reach the test process's mock servers via `host.docker.internal` (mapped through `host-gateway` on Linux), the same mechanism the existing suites use via `ANDROMEDA_TEST_CALLBACK_HOST`.
- **Source identity extras**: every broker reads `[federation.identity.extras] deployment = "distop"` from the mounted `andromeda.toml`, so `/info/sourceIdentity` carries a deployment-specific member (clause 5.2.40).
