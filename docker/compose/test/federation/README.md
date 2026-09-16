# Federation acceptance topology

This stack is intentionally separate from `andromeda-distributed`. It has one federation hub, four independent domain brokers, and one PostgreSQL server per broker identity. No broker can observe another broker's store except through the NGSI-LD HTTP APIs.

Start it with:

```sh
docker compose -f docker/compose/test/federation/docker-compose.yaml up --build -d
```

The host-facing broker ports are `1071` through `1075` and PostgreSQL ports are `5451` through `5455`. The peer endpoints used in registrations are the compose names at those same broker ports. The `fed` test client maps those names to `127.0.0.1` and the published port without rewriting the discovered URL.

Stop it, including test data, with:

```sh
docker compose -f docker/compose/test/federation/docker-compose.yaml down -v
```
