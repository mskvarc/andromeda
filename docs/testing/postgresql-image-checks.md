# PostgreSQL image checks

The scripts in `docker/images/postgresql/tests` start the PostgreSQL image in Docker, most of them with a broker beside it, and check what ends up in the database and in the broker log. Each script names its containers after its own process id and removes everything it created when it exits. Every script except `settings-check.sh` also runs on a Docker network of its own. A script prints one `ok` or `FAIL` line per assertion and exits non-zero if any assertion failed.

Only `settings-check.sh` runs in CI. The `postgresql` workflow runs it once per edition and platform, before the image is pushed (see [Continuous integration](../continuous-integration.md)). You run the others by hand, against the images built from the working tree. The checks with a broker need `curl` on the host, and `repack-check.sh` also needs `python3`.

## Building the images

Build both images from the repository root.

```sh
docker build -t andromeda-postgresql:local docker/images/postgresql
docker build -t andromeda:local -f docker/images/andromeda/Dockerfile .
```

The PostgreSQL image is the TSL edition unless you build it with `--build-arg TIMESCALEDB_EDITION=apache`. Every check that needs a columnstore takes the TSL edition.

## The checks

Every check except `settings-check.sh` takes the PostgreSQL image first and the broker image second.

```sh
bash docker/images/postgresql/tests/recompression-check.sh andromeda-postgresql:local andromeda:local
```

| Script | Checks |
| --- | --- |
| `settings-check.sh IMAGE [docker-run-args...]` | The settings the entrypoint and `timescaledb-tune` produce under container limits, after it confirms the tuner is the pinned version whose values it expects. That covers memory, workers and `work_mem` at 2GB and 8GB, `shared_buffers` with no limit set, the `TS_TUNE_MEMORY`, `TS_TUNE_NUM_CPUS` and `TS_TUNE_MAX_BG_WORKERS` overrides, the default 200 connections and an explicit 400, one CPU, WAL sizes on a shared and a dedicated WAL volume with the caps on a large one, `temp_file_limit` on a small volume, `timescaledb.license` for each edition, the pinned TimescaleDB Toolkit in `tsl` and its absence in `apache`, and `pg_stat_statements.max`. It also checks the fixed policy file and the derived file each setting comes from, that `ALTER SYSTEM` survives a restart, the memory budget and `/dev/shm` warnings, that no `POSTGRES_` variable is left in the server's environment, the default `andromeda` role and database, the `POSTGRES_USER`, `POSTGRES_USER_FILE` and `POSTGRES_DB` overrides, init scripts, and that a command other than `postgres` leaves the configuration alone. Extra arguments go to every `docker run`. |
| `schema-storage-check.sh` | The storage decisions that the migrations and the storage-policy reconciler leave in a new tenant database. These are the columnstore sparse indexes, the BRIN parameters of history, the vacuum parameters of the queue, current-state and history tables, cached identity sequences, the absence of any TimescaleDB conversion job, and the session defaults on the tenant and platform databases. After a restart, the broker log has to stay clean, which means the reconciler found nothing to correct. |
| `columnstore-conversion-check.sh` | That converting history needs no TimescaleDB background worker. On a server that allows one, two tenants convert an aged chunk on the broker's own sweep, keep a chunk with too few rows per stream in the rowstore, install no job, and keep every row readable. |
| `extension-update-check.sh [PREVIOUS_IMAGE PREVIOUS_BROKER_IMAGE]` | That opening a tenant updates its TimescaleDB, PostGIS and pg_repack to the versions the image ships, and that the tenant serves afterwards. Given the previous release's images, it also checks that twenty tenants the previous broker created and filled on the previous image are brought up to date within 90 seconds with no request, the one with the most committed transactions among the first eight opened. The recorded versions have to match each tenant database's `pg_extension`, and a request afterwards must run no further update. |
| `recompression-check.sh` | That automatic maintenance recompresses a converted chunk that an `UPDATE` left partial, and records the action as succeeded on the live branch. It also checks that the fleet observation reports a database shaped like a tenant's that no registry row names. |
| `maintain-privilege-check.sh` | That automatic maintenance, run as a role that isn't a superuser and lacks `MAINTAIN` on the tenant's `entities`, still observes the live branch instead of skipping the tenant. The visit records its own outcome (`would-act`) and its catalog samples, logs a `capability-unavailable` warning naming the live branch, reports `autovacuum-overdue` and `statistics-stale`, and records no action result or queued action there. |
| `action-right-check.sh` | That automatic maintenance gives up only the actions whose own right it lacks. It runs as a role that owns no tenant table and holds `MAINTAIN` on every table of `public` but on no history chunk. A partial chunk and a kept rowstore chunk waiting for its rebuild are recorded as `would-act` with no result, queued action or statement, the chunk stays partial, and no chunk is recorded as rebuilt. An invalid `idx_ca_value_number_ccnew` is logged as `would-drop-artifact` and left in place. Each missing right is logged as `capability-unavailable` with its relation, and the queued analyze of `entities` still runs. |
| `pgstattuple-absence-check.sh` | That automatic maintenance of the default tenant runs everything but the index probes when `pgstattuple` is dropped after startup and autovacuum is off on `current_attributes`. It reports `autovacuum-overdue` and runs the catch-up vacuum, and recompresses a converted chunk that an `UPDATE` left partial. It records no probe, rebuild or queued rebuild of a B-tree or GiST index, although `pk_ca_grain` has grown past the probe's size floor, and it logs exactly one `capability-unavailable` warning naming the missing extension. |
| `repack-check.sh` | That automatic maintenance repacks tables online and safely, in the cases listed below the table. |
| `history-wake-check.sh [APACHE_IMAGE]` | That a tenant no request loads still has its history converted and aged out. After the broker restarts, with no request to the tenant, the wake loop loads it and drops a chunk past `temporal.storage.retention.delete_after`, or, with retention disabled, converts a chunk past `temporal.storage.columnstore.convert_after`. Once the tenant unloads again, its next wake is recorded weeks ahead. Given the Apache edition image, it also checks that a tenant with no conversion and no retention due gets no wake. |

`repack-check.sh` covers these cases.

- A bloated `notification_deliveries` is repacked online under a concurrent writer. Half its size is given back, its earlier rows are unchanged, every row the writer added is kept, no repack object is left behind, and the size before and after is recorded.
- The objects a broker leaves when it's killed mid-run are dropped by the next broker and recorded as `repack-recovered`.
- A writer that holds the table through the swap makes the run give up after its lock budget, without being cancelled.
- A role without `pg_repack` only reports the repack. A role granted the `repack` schema but not the `repack.tables` view is reported as missing a right, and the repack runs once both are granted.
- A Snapshot branch's `current_attributes` is repacked in its own schema.
- The live `current_attributes` comes back in key order and reads fewer buffers per entity.
- A waiting `ALTER TABLE ... ADD COLUMN ... DEFAULT` makes the run yield within 15 seconds, and the run then completes.
- Another tenant's queued analyze runs while a repack is held open.
- The wake catch-up reinstalls a `pg_repack` that was dropped without being recorded.
- The p99 write latency on `current_attributes` stays within 250 ms of its baseline during a repack.
- The extension accepts every repack target, and each target keeps its identity sequence, its constraints and the foreign keys into it across a swap.
- A run holds the table's `SHARE UPDATE EXCLUSIVE` lock on its guard session through its replay.
- A waiting `CREATE INDEX` makes the run yield, and the index stays consistent with the heap through the next repack.
- A broker stopped mid-run cleans up before it exits, within 40 seconds.
- A repack that a gate holds back doesn't keep another tenant's repack from running.
- A bulk insert committed while the swap waits for its lock is replayed with the lock given back, so no concurrent write waits two seconds.
- In four of the cases, every repacked index passes `bt_index_check` with every heap tuple indexed.

## How long they take

Most of these checks wait for the broker's maintenance to come round on its own, so they take minutes, not seconds.

- `recompression-check.sh` waits up to 15 minutes. The chunk qualifies after a three-minute breach run and is recompressed on the tenant's next visit for its queued action.
- `maintain-privilege-check.sh` waits up to 5 minutes for the tenant's first visit to finish.
- `action-right-check.sh` waits up to 15 minutes for the analyze, which qualifies after a three-minute breach run and runs on a later visit. It then waits up to 5 minutes for each history chunk's `would-act` decision, since that chunk's own breach run may not have lasted three minutes yet.
- `pgstattuple-absence-check.sh` waits up to 15 minutes for the vacuum and up to 15 more for the recompression. Each qualifies after a three-minute breach run and runs on a later visit.
- `history-wake-check.sh` waits up to 5 minutes for each wake and each unload, and runs its cases one after another.
- `repack-check.sh` queues most of its repacks directly. Where a case needs a breach run, it seeds one seven hours back, so the responsive profile's six-hour qualifying duration has already passed. Each wait in a case lasts up to 15 minutes, and a case can have several. To hold a run at its replay, it leaves a transaction open and idle from before the run's copy, which the swap then waits out.

## When a check fails

A failed assertion prints a `FAIL` line naming what it checked, and many assertions also print what they expected and what they found. The broker log stays in its container until the script exits. To look into a failing check, run it again with the `cleanup` trap removed, so the containers stay behind.
