# Snapshot acceptance topology

The `andromeda-snapshot` Compose project provides a primary broker on port 1081, a replica of the same broker identity and database on port 1082, and an independent remote source broker on port 1083. The shared and remote PostgreSQL servers are published on ports 5481 and 5482.

Start it with `docker compose -f docker/compose/test/snapshot/docker-compose.yaml up --build --wait`.

Run the suite with `ANDROMEDA_TEST_BROKER_URL=http://127.0.0.1:1081/ngsi-ld/v1 ANDROMEDA_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5481/snapshot_primary ANDROMEDA_TEST_CALLBACK_HOST=host.docker.internal cargo test --test snapshot -- --test-threads=1`. The host-side acceptance client uses typed DNS overrides for `snapshot-primary`, `snapshot-replica`, and `snapshot-remote`, mapping them to their published loopback ports so advertised URLs remain identical inside and outside Compose.
