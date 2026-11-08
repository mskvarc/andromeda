# Online repack

Vacuum makes the space of removed rows reusable inside a table's pages, but it never gives the pages back. A work queue that a burst filled and that then drained, or a `current_attributes` heap that a mass deletion thinned out, keeps its size on disk until it's rewritten. The maintenance controller rewrites such a heap online, through the `pg_repack` extension. A repack goes through the same gates, cooldowns, rights checks, durable state and telemetry as every other maintenance action. This page describes how a run works and why it's built the way it is.

## Driving the extension

The controller runs the `pg_repack` protocol in process, over its own sessions, against the extension's SQL interface. That interface is the `repack.tables` view, which generates every statement that names the table's columns, key and storage, and the functions `repack.repack_indexdef`, `repack.repack_apply`, `repack.repack_swap`, `repack.repack_drop`, `repack.conflicted_triggers`, `repack.get_order_by`, `repack.create_table` and `repack.version`. The extension still writes every table-specific statement and performs the swap. The controller owns only the orchestration, which it has to own anyway, meaning the gates, the locking policy, cancellation, the statement budgets and recovery.

Running the `pg_repack` command line instead was rejected, for several reasons.

- It would be the first child process of any production crate.
- The client has to be exactly the extension's version, which the broker image can't know for a server image or a managed server.
- Credentials would have to be handed to a child process.
- A killed child leaves its trigger behind, logging every write, and nothing in the broker would know it exists.
- Its timeouts and cancellation would bypass the controller's own.

`pg_squeeze` needs `wal_level = logical` and preloading, and PostgreSQL 18 has no `REPACK` command, so neither is an option. An index-only repack isn't needed, because `REINDEX CONCURRENTLY` covers it.

The coupling to the extension's internals is kept small. The image pins the extension's version. The controller drives one release line, 1.5, and treats any other version as a missing capability rather than a run that could fail halfway. The protocol below was checked against the source of the pinned release, 1.5.3, and walked by hand on the image. A table with an identity key, a stored generated column, a check constraint, a foreign key in each direction, a statement-level trigger and a fillfactor keeps all of them across a swap, and its identity sequence and incoming foreign keys work afterwards. The controller implements the protocol from that reading, and copies no code from the extension, which is BSD-licensed.

## Targets and copy order

Every allowlisted heap is a repack target. They are `current_attributes`, `entities`, `subscription_delivery_state`, `entity_search`, `temporal_attribute_streams` and the five work queues. All of them have a primary key, which the log needs to identify a row. The history hypertables and the census tables are never targets. History chunks are append-only and columnstore conversion rewrites them anyway, and a repack of a hypertable's parent doesn't touch its chunks. A branch heap is a target in the live branch and in every published Snapshot branch.

`current_attributes` is copied in the order of its grain key, so the attributes of one entity, which every entity read fetches together, share pages, as `CLUSTER` would leave them. Every other heap is copied in the order its rows are read. Each of them is a queue, a catalogue with one row per key, or a projection read one key at a time, where order buys nothing.

## When a table qualifies

A repack is judged on the free space of the heap and of its TOAST table together, as `pgstattuple_approx` estimates it. Entity bodies and notification payloads are stored out of line, so the TOAST table is often where the space is. The estimate reads only the pages the visibility map doesn't mark all-visible, so it stays cheap on a settled heap. It runs within the visit's probe budget, after every index probe, and only for heaps past the profile's size floor and out of their repack cooldown.

| | Conservative | Responsive |
|---|---|---|
| Size floor, heap and TOAST | 256 MiB | 64 MiB |
| Bytes to give back | 128 MiB | 32 MiB |
| Free share | 50 % | 35 % |
| Qualifying duration | 24 h | 6 h |
| Cooldown after a repack | 7 d | 2 d |

The thresholds are tested in a fixed order.

1. The size floor.
2. The configured `database.maintenance.max_repack_size`.
3. Dead tuples at or above the profile's vacuum threshold. Vacuum turns them into free space first, and the repack is judged on what it leaves.
4. The free share.
5. The bytes to give back.

The long qualifying duration is the hysteresis. A queue that drains and refills on its own never stays above the free threshold for a whole day.

The target's cooldown and the load gates decide before any threshold, and so do the repack's own gates. A repack waits while a run of the table is already in progress or was left behind, while a replication slot keeps more WAL on disk than the relation takes (a repack writes the relation's size in WAL again), and while there is freeze pressure on the database, since the copy holds the database's freeze horizon for as long as it reads. In a Snapshot branch, it also waits when the branch's remaining lifetime is shorter than the run's budget plus the 30-minute margin every maintenance statement in a Snapshot branch keeps. While a repack's trigger is on a table, the table's vacuum, analyze and index rebuilds wait too.

A repack is decided under `database.maintenance.mode`, except that automatic mode only observes repacks until `database.maintenance.repack` is `"performed"`. Unlike the controller's other actions, a repack is only reported by default. A run needs free disk of up to twice the table's size, which no SQL can measure, so an operator sees the repacks the controller would run before any of them runs. A repack whose extension is missing or on another release line, or which the role isn't allowed to run, is decided under observe mode, and one queued earlier is withdrawn.

## Who runs a repack

A maintenance pass only decides and queues. A run takes minutes or hours, and inside a pass it would hold the pass's fleet lock all that time, keeping every other tenant from its vacuums, rebuilds and freeze reports. So the `postgresql-repack` task runs beside the pass, on the pass's cadence, in every broker process where maintenance isn't disabled. Unless the controller is automatic and repacks are performed, every execution only observes. Each execution takes a fleet lock of its own, so only one repack runs at a time across all brokers while the passes go on.

The task goes through the queued repacks that are past their backoff, in order. It judges each one again on a fresh sample, against gates measured at that moment, the way a queued vacuum is judged again before it runs, and it runs the first one that gets through. A repack held back where it is, by a gate, an unreachable or outdated database, a tenant no longer registered or a table another run holds, stays queued and gives the next one its turn, so it never holds back the rest of the fleet. The queued action on the target row is the hand-over. No state passes between processes in memory, so any broker can run what another queued.

## A run

A run opens two sessions of its own into the tenant database, straight to PostgreSQL. Both check their client every second (`client_connection_check_interval`), so a broker that stops mid-statement stops the statement too.

The main session copies, builds and replays. The guard session holds the table's `SHARE UPDATE EXCLUSIVE` lock from the setup to the swap. That lock conflicts with every schema change, vacuum, analyze and concurrent index build, and with no read or write, so the table keeps the shape the copy was made from. The guard sits idle in its transaction for the whole run, so its session lifts the idle-in-transaction timeout. It runs no snapshot between statements, so it holds no vacuum horizon.

The main session takes the same session advisory lock the command line takes for the table, keyed on `16185446` and the table's OID shifted by -2^31. So the controller and an operator's command never repack the same table at once, and recovery never cleans up after a run that is still going.

1. **Setup.** The guard opens its transaction first. Then, in one read-committed transaction on the main session, under the table's `ACCESS EXCLUSIVE` lock taken in bounded attempts, the run refuses a table with an invalid index or a trigger with the run's name. It reads the generated statements and every valid index's definition rewritten for the copy, creates the key type, the log table and the trigger, enables the trigger for every session, and keeps autovacuum off the log. The run's platform row is written before the setup commits. The guard then queues its lock request in its own transaction, which keeps the lock until the swap commits it. Once the request shows as waiting in `pg_locks`, the setup commits, so the guard is next in line and no schema change slips in between. If a request that conflicts with the guard is already waiting, it's ahead of the guard in the queue. The setup then rolls back, and the run yields instead of cancelling it.
2. **Copy.** This is one repeatable-read transaction, with `work_mem` raised to `maintenance_work_mem`. Its first statement reads the virtual transactions open at its snapshot, apart from the run's own sessions, autovacuum and manual vacuums. It then empties the log, takes the table's `ACCESS SHARE` lock in bounded attempts, creates the copy table in the table's tablespace, sets its column storage, copies every row (in key order when the target is clustered), drops the columns the table has dropped, and keeps autovacuum off the copy. Emptying the log before the copy reads means every row is either copied or logged after the copy, never both.
3. **Index build.** Each rewritten definition runs in its own transaction.
4. **Replay.** The log is replayed onto the copy in batches of 1,000 rows until fewer than 100 remain. Then the run waits out the transactions captured at the copy's snapshot, replaying the log meanwhile, so the replay before the swap sees every write they made.
5. **Swap.** On the guard's transaction, inside a savepoint, the run takes `ACCESS EXCLUSIVE` on the table and the copy, in bounded attempts with the log replayed between them. It replays the rest of the log and calls `repack.repack_swap`, which gives the table the copy's files and indexes, and the guard's transaction commits. Writes committed while the lock was awaited, a bulk insert for example, can leave more in the log than the 1,000 rows a short replay takes. The savepoint is then rolled back, which gives the lock back, and the log is replayed down to under 100 rows before the lock is tried again, so no writer waits behind the swap for a long replay.
6. **Drop.** On the main session, under the table's `ACCESS EXCLUSIVE` lock, `repack.repack_drop` removes all four objects, which are the key type, the log, the trigger and the copy table, now holding the old files. The platform row is deleted.
7. **Analyze.** The table's planner statistics are refreshed. A failure here is logged and leaves the outcome as it is.

Each phase is recorded on the platform row and timed in `andromeda_postgresql_maintenance_repack_phase_seconds`. The copy and each index build run under a `statement_timeout` of the relation's size at 8 MiB/s, kept between 30 minutes and 6 hours.

### Locks under writes

A lock request waits in the table's queue ahead of every later request, so while an `ACCESS EXCLUSIVE` request waits, every writer of the table queues behind it. Each attempt therefore waits at most 200 ms, which caps the stall a write sees. Attempts are spaced by a back-off that grows by 100 ms up to a second, and every other one is replaced by a replay of the log. After 60 seconds the run gives up. No other backend is ever cancelled to make room for it.

### Yielding

From the setup to the swap, a concurrent task on the guard session looks at `pg_locks` every second. A session can wait more than 5 seconds for a lock on the table because the run's own sessions block it, as `pg_blocking_pids` reports. Examples are a tenant migration's `ALTER TABLE` or `CREATE INDEX` on rollout, an operator's DDL, or an anti-wraparound vacuum. The run then cancels its own statement with `pg_cancel_backend` and cleans up, so the waiting session gets the table without the run's objects on it. Otherwise the migration would wait for the whole copy, and every write of the table would wait behind the migration. A session that waits on the table for someone else's lock isn't yielded to, because giving the table up wouldn't let it through.

### Giving up, failing and stopping

A run that stops before its swap, for any reason, or whose swap isn't granted its lock in time, cleans up on the guard's transaction first. The guard already holds the table's `SHARE UPDATE EXCLUSIVE` lock, so its request for `ACCESS EXCLUSIVE` goes ahead of a schema change waiting for the table, and the run's objects are gone before the schema change reaches the table. The order matters. While the run's log table has a column of the table's row type, PostgreSQL refuses an `ALTER TABLE` that adds a column with a default or changes a column's type ("cannot alter table because column `log_<oid>.row` uses its row type"), so a migration let through first would fail. The main session's statement is cancelled and its transaction rolled back before the guard asks for the lock, so its own locks don't hold the guard up. A guard that doesn't clean up rolls its transaction back, so its own lock doesn't hold up the cleanup that follows.

When the guard can't clean up, what happens depends on how the run ended.

- A run that gave up on its own cleans up on its main session, which still holds the table's advisory lock. It gives up when a lock isn't granted in time, when writes keep the log from shrinking for ten minutes, or when a transaction older than the copy stays open for five.
- A run that yielded, failed or was cancelled closes both sessions, which ends their statements and transactions. It cleans up on a fresh session once the closed one has released the table's advisory lock.
- A setup that fails, which may have committed before the guard's lock failed, cleans up on its main session.

A cancelled run gets 30 seconds of the task's 60-second shutdown budget for its whole cleanup, on the guard and after it. When the broker is stopping, the stop deadline (`server.graceful_drain_timeout` plus `notifications.evaluation.drain_timeout`, counted from the stop signal) ends the task too, so with `notifications.evaluation.drain_timeout` below 30 seconds the cleanup can be cut off before its 30 seconds are up. Whatever a cleanup leaves behind, the platform row keeps naming, and recovery cleans it up on the next visit.

## Recovery

Recovery comes first in every visit, before the recovery of leftover concurrent-index artifacts. Leftovers are found from the tenant database's catalogue alone, as a `repack.log_<oid>` or `repack.table_<oid>` table, or a `repack_trigger` on a table. So a run whose platform row is gone is recovered too, and so is an operator's interrupted command-line run on an allowlisted table. Objects on any other table are reported for the operator.

A table's objects are cleaned up only while nobody holds its advisory lock, under its `ACCESS EXCLUSIVE` lock taken in bounded attempts, by `repack.repack_drop`. The target row records `repack-recovered` and the failure back-off. A recorded run with nothing left in the database is forgotten once no repack runs anywhere. While no broker holds the repack fleet lock, every recorded run belongs to a run that is no longer going, and triage visits its tenant before any other, because the trigger keeps doubling the table's writes until recovery runs.

## The extension's lifecycle

The broker installs `pg_repack` in every tenant database whose role may create it. The extension is untrusted, so that role has to be a superuser. It ships no update scripts, so in a database behind the version the server ships, it's dropped with `RESTRICT` and created again, under the extension update lock. `RESTRICT` fails while a run's objects depend on the `repack` schema. The failure is logged and the next open tries again, so the lifecycle and the maintenance controller share no lock. The tenant extension version record tracks `pg_repack` beside PostGIS and TimescaleDB, so the wake pass loads a tenant whose `pg_repack` is behind the image, or absent and not yet attempted against the version the image ships.

## Measuring a run

A repack is measured by the relation's total bytes, its indexes and TOAST table included, before and after. It counts as effective once it gives back at least half the profile's reclaim floor. One that gives back less, because writes refilled the space while it copied, cools down for the longer of the repack cooldown and the profile's success cooldown. A run that gave up records why (`repack-abandoned-lock-wait`, `repack-not-converging`, `repack-yielded`) and backs off like a failure.
