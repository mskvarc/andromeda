# Distributed Andromeda topology

Real multi-instance Andromeda for validating NGSI-LD distributed operations (Context Source Registration, forwarding, federation, entity maps) end-to-end over HTTP. This is the system-level counterpart to the in-process `distop_*` suites, which exercise forwarding against in-process mock Context Sources.

See [`docs/distributed-operations-test-plan.md`](../../docs/distributed-operations-test-plan.md) for how the suite uses this topology and [`docs/distributed-operations-conformance-register.md`](../../docs/distributed-operations-conformance-register.md) for the requirement register the suite implements.

## Topology

Three broker instances, one shared TimescaleDB/PostGIS Postgres (one base database per broker, created by [`initdb/10-create-broker-databases.sql`](initdb/10-create-broker-databases.sql)). Each instance has a distinct `INFO_SOURCE_ID` / `INFO_SOURCE_ALIAS`, which is what makes `Via` / `contextSourceAlias` loop detection meaningful across instances.

| Instance | Host URL (test client) | Peer URL (in CSR `endpoint`) | Alias |
|---|---|---|---|
| broker-a | `http://127.0.0.1:1061/ngsi-ld/v1/` | `http://broker-a:1056` | `broker-a` |
| broker-b | `http://127.0.0.1:1062/ngsi-ld/v1/` | `http://broker-b:1056` | `broker-b` |
| broker-c | `http://127.0.0.1:1063/ngsi-ld/v1/` | `http://broker-c:1056` | `broker-c` |

The two URL families matter: the test client on the host reaches brokers via published `localhost` ports, but a registration's `endpoint` must be the compose-network name, because one broker calls another over that network — never via `localhost`.

## Bring up / tear down

```sh
# build the broker image once and start the stack
docker compose -f docker/distributed/docker-compose.distributed.yaml up --build -d

# wait until all three report healthy
docker compose -f docker/distributed/docker-compose.distributed.yaml ps

# logs for one instance
docker compose -f docker/distributed/docker-compose.distributed.yaml logs -f broker-a

# stop and remove (Postgres data is ephemeral — no named volume)
docker compose -f docker/distributed/docker-compose.distributed.yaml down -v
```

The broker image (`docker/andromeda/Dockerfile`) builds a debug binary and ships no config file — every setting comes from `ANDROMEDA_*` env vars, which all have defaults. The Postgres image (`docker/postgresql/Dockerfile`) provides TimescaleDB + PostGIS, required by the tenant migrations.

## Notes

- **Fault injection** (timeouts, `403`, malformed payloads, loops, unsupported operations) is not driven from these real peers — a real Andromeda peer answers correctly by construction. Those cases use the in-process `ContextSourceServer` mock as the registered peer, per the plan. This stack is for genuine broker-to-broker federation semantics; the mock is for controllable adversarial peers.
- **Mock reachability**: brokers reach the test process's mock servers via `host.docker.internal` (mapped through `host-gateway` on Linux), the same mechanism the existing suites use via `ANDROMEDA_TEST_CALLBACK_HOST`.
- **Per-broker Postgres** is an alternative to the shared-Postgres default if you want stricter isolation; swap each broker's `ANDROMEDA_DATABASE_URL` to its own Postgres service.
