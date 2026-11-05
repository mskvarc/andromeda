#!/usr/bin/env bash
# usage: repack-check.sh IMAGE BROKER_IMAGE
# Asserts that automatic maintenance repacks a bloated table online and
# recovers from every way a run can stop short:
#
#  1. a bloated notification_deliveries is repacked under a concurrent
#     writer: the relation shrinks by half, the rows it held before are
#     unchanged, every writer row is there, no repack object is left, and the
#     target row records the success with its bytes before and after;
#  2. a broker killed in the middle of a run leaves objects that the next
#     broker's visits drop, recording the recovery;
#  3. a writer holding the table through the swap makes the run give up
#     after its lock budget without cancelling the writer, and the objects
#     are dropped once the writer is gone;
#  4. a role that may not create pg_repack only reports repacks, a role
#     granted the repack schema but not the repack.tables view is reported as
#     missing a right, and once granted both, the repack runs;
#  5. a heap of a published Snapshot branch is repacked in its branch schema;
#  6. current_attributes is written back in key order: entity_key correlates
#     at 0.99 or more and fetching a hundred entities' attributes reads fewer
#     buffers than before;
#  7. an ALTER TABLE waiting for the table makes the run yield within ten
#     seconds, and the ALTER, which adds a column with a default, completes;
#  8. a queued analyze of another tenant runs while a repack is held open;
#  9. a tenant whose pg_repack is gone and unrecorded gets it back from the
#     wake catch-up after a restart;
# 10. p99 write latency on current_attributes during a repack stays within
#     250 ms of its baseline;
# 11. every repack target table is one the extension accepts, and each keeps
#     its identity sequence, its constraints and the foreign keys into it
#     across a swap;
# 12. a run held at its replay holds the table's SHARE UPDATE EXCLUSIVE lock
#     on its guard session;
# 13. a CREATE INDEX waiting for the table makes the run yield, and the
#     index it builds is consistent with the table;
# 14. a broker stopped in the middle of a run cleans up within its shutdown
#     budget;
# 15. a queued repack a gate holds back does not keep another tenant's
#     queued repack from running;
# 16. a bulk insert committed while the swap waits for its lock is replayed
#     with the lock given back: a concurrent writer never waits seconds.
#
# Every repack's indexes are checked against its heap with amcheck.
#
# Runs are held at the replay phase deterministically by a transaction that
# is open, idle and without a snapshot when the copy starts: the swap waits
# for every transaction older than the copy, and such a transaction trips no
# long-transaction gate. Breach runs are seeded seven hours back, so the
# responsive profile's six-hour qualifying duration has passed and the next
# sample queues the repack. Each case waits up to fifteen minutes.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="repack-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
owner_broker="$prefix-owner-broker"
role=repack_owner
tenant=rpk
other=rpb
failures=0
holders=()

cleanup() {
	for holder in "${holders[@]}"; do
		kill "$holder" > /dev/null 2>&1 || :
	done
	docker rm -f "$broker" "$owner_broker" "$database" > /dev/null 2>&1 || :
	docker network rm "$network" > /dev/null 2>&1 || :
}
trap cleanup EXIT

pass() {
	printf 'ok   %s\n' "$1"
}

fail() {
	printf 'FAIL %s\n' "$1" >&2
	failures=$((failures + 1))
}

# usage: expect DESCRIPTION EXPECTED ACTUAL
expect() {
	if [ "$2" = "$3" ]; then
		pass "$1: $3"
	else
		fail "$1: expected '$2', got '$3'"
	fi
}

# usage: expect_true DESCRIPTION ACTUAL
expect_true() {
	expect "$1" t "$2"
}

# usage: sql DATABASE QUERY
# The image bounds every statement; the check's own long loops and bulk
# writes run unbounded.
sql() {
	docker exec -u postgres "$database" psql -XAtq -v ON_ERROR_STOP=1 -d "$1" -c 'SET statement_timeout = 0' -c "$2"
}

# usage: sql_steps DATABASE STATEMENT...
# Runs each STATEMENT on its own in one session, so a procedure may commit
# and a temporary table lasts from one statement to the next.
sql_steps() {
	local database_name="$1" steps=(-c 'SET statement_timeout = 0') statement
	shift
	for statement in "$@"; do
		steps+=(-c "$statement")
	done
	docker exec -u postgres "$database" psql -XAtq -v ON_ERROR_STOP=1 -d "$database_name" "${steps[@]}"
}

# The check's own bloat rows, as a repack must leave them: the broker's
# deliveries change with every attempt the unreachable endpoint fails, and
# the hold that failure puts on the subscription stamps held_at on every
# delivery of it.
check_rows="SELECT delivery_key, event_key, subscription_key, next_attempt_at, attempt_count, lease_until, delivered_at, exhausted_at, claimed_by, last_error
	FROM notification_deliveries WHERE last_error = repeat('x', 300)"

# usage: keep_rows DATABASE
# Saves the check's rows as they are now.
keep_rows() {
	sql "$1" "DROP TABLE IF EXISTS repack_check_kept; CREATE TABLE repack_check_kept AS $check_rows" > /dev/null
}

# usage: rows_changed DATABASE
# How many kept rows are gone or changed, and how many rows are new, as
# 'GONE NEW'; the first few differences go to the log.
rows_changed() {
	sql "$1" "SELECT 'gone', * FROM (TABLE repack_check_kept EXCEPT ALL ($check_rows)) g UNION ALL SELECT 'new', * FROM (($check_rows) EXCEPT ALL TABLE repack_check_kept) n ORDER BY 3 LIMIT 5" >&2
	sql "$1" "SELECT (SELECT count(*) FROM (TABLE repack_check_kept EXCEPT ALL ($check_rows)) g) || ' ' || (SELECT count(*) FROM (($check_rows) EXCEPT ALL TABLE repack_check_kept) n)"
}

# usage: grant_role DATABASE
# The data privileges on every table and sequence of every schema the broker
# writes, and CREATE on public, which the migrator needs to confirm its ledger
# table exists.
grant_role() {
	sql "$1" "DO \$\$
	DECLARE
		schema_name name;
	BEGIN
		FOR schema_name IN
			SELECT nspname FROM pg_namespace
			WHERE nspname NOT LIKE 'pg\\_%' AND nspname NOT IN ('information_schema', 'repack')
		LOOP
			EXECUTE format('GRANT USAGE ON SCHEMA %I TO $role', schema_name);
			EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA %I TO $role', schema_name);
			EXECUTE format('GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA %I TO $role', schema_name);
		END LOOP;
		GRANT CREATE ON SCHEMA public TO $role;
	END
	\$\$;" > /dev/null
}

# usage: wait_for DESCRIPTION EXPECTED SECONDS DATABASE QUERY
wait_for() {
	local actual=''
	for _ in $(seq 1 "$3"); do
		actual="$(sql "$4" "$5")"
		[ "$actual" = "$2" ] && break
		sleep 1
	done
	expect "$1" "$2" "$actual"
}

# usage: start_postgres CONTAINER
start_postgres() {
	docker run -d --name "$1" --network "$network" -e POSTGRES_PASSWORD=check "$image" > /dev/null
	for _ in $(seq 1 120); do
		if docker exec "$1" pg_isready -q -h 127.0.0.1 2> /dev/null; then
			return
		fi
		sleep 0.5
	done
}

# usage: start_broker CONTAINER DATABASE_URL
start_broker() {
	docker run -d --name "$1" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__URL=$2" \
		-e ANDROMEDA__DATABASE__MAINTENANCE__MODE=automatic \
		-e ANDROMEDA__DATABASE__MAINTENANCE__PROFILE=responsive \
		-e ANDROMEDA__DATABASE__MAINTENANCE__REPACK__MODE=automatic \
		-e ANDROMEDA__DATABASE__MAINTENANCE__REPACK__MAX_RELATION_SIZE=unlimited \
		-e ANDROMEDA__NOTIFICATIONS__DELIVERY__MODE=durable \
		"$broker_image" > /dev/null
	wait_healthy "$1"
}

# usage: wait_healthy CONTAINER
wait_healthy() {
	local base
	base="http://$(docker port "$1" 1056/tcp)/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
			return
		fi
		sleep 0.5
	done
}

# usage: api CONTAINER TENANT METHOD PATH [BODY]
api() {
	local base
	base="http://$(docker port "$1" 1056/tcp)/ngsi-ld/v1"
	curl -sS -o /dev/null -w '%{http_code}' -X "$3" -H "NGSILD-Tenant: $2" -H 'Content-Type: application/json' ${5:+--data "$5"} "$base$4"
}

# usage: tenant_database TENANT
tenant_database() {
	sql postgres "SELECT database_name FROM tenants WHERE name = '$1'"
}

# usage: tenant_key TENANT
tenant_key() {
	sql postgres "SELECT tenant_key FROM tenants WHERE name = '$1'"
}

# usage: open_holder DATABASE STATEMENTS
# Opens a session that runs STATEMENTS and then stays open, idle in its
# transaction; prints the holder's process id. close_holder ends it.
open_holder() {
	local fifo
	fifo="$(mktemp -u)"
	mkfifo "$fifo"
	PGOPTIONS='-c statement_timeout=0' docker exec -i -e PGOPTIONS -u postgres "$database" psql -XAtq -d "$1" < "$fifo" > /dev/null 2>&1 &
	holders+=("$!")
	exec {holder_fd}> "$fifo"
	rm -f "$fifo"
	printf '%s\n' "$2" >&"$holder_fd"
}

# usage: close_holder STATEMENT
close_holder() {
	printf '%s\n' "$1" >&"$holder_fd"
	exec {holder_fd}>&-
}

# usage: bloat_deliveries DATABASE ROWS
# Clones the tenant's one notification event ROWS times, gives each clone a
# delivery that is not due for ten years, then deletes nine in ten of those
# deliveries and vacuums, which leaves their space free inside the heap. The
# deliveries go to the bloat subscription, which matches no entity: none of
# them is ever due, so its endpoint never fails and no hold or probe of a
# failing subscription touches them.
bloat_deliveries() {
	sql "$1" "DO \$\$
		DECLARE cols text; vals text;
		BEGIN
			SELECT string_agg(quote_ident(column_name), ', ' ORDER BY ordinal_position),
			       string_agg(CASE column_name WHEN 'write_id' THEN 'gen_random_uuid()' ELSE 'e.' || quote_ident(column_name) END, ', ' ORDER BY ordinal_position)
			  INTO cols, vals
			  FROM information_schema.columns
			 WHERE table_schema = 'public' AND table_name = 'notification_events' AND is_identity = 'NO' AND is_generated = 'NEVER';
			EXECUTE format('INSERT INTO notification_events (%s) SELECT %s FROM (SELECT * FROM notification_events ORDER BY event_key LIMIT 1) e CROSS JOIN generate_series(1, $2)', cols, vals);
		END \$\$" > /dev/null
	sql "$1" "INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at, last_error)
		SELECT e.event_key, s.subscription_key, now() + interval '10 years', repeat('x', 300)
		FROM notification_events e CROSS JOIN (SELECT subscription_key FROM subscriptions WHERE id = 'urn:ngsi-ld:Subscription:bloat') s
		ON CONFLICT DO NOTHING" > /dev/null
	sql "$1" "DELETE FROM notification_deliveries WHERE last_error = repeat('x', 300) AND delivery_key % 10 <> 0" > /dev/null
	sql "$1" "VACUUM notification_deliveries" > /dev/null
}

# usage: seed_breach TENANT TARGET SNAPSHOT_KEY
seed_breach() {
	sql postgres "INSERT INTO postgresql_maintenance_targets (tenant_key, snapshot_key, target, first_breach_at, breach_samples)
		SELECT tenant_key, $3, $2, now() - interval '7 hours', 1 FROM tenants WHERE name = '$1'
		ON CONFLICT (tenant_key, snapshot_key, target) DO UPDATE SET first_breach_at = excluded.first_breach_at, breach_samples = 1,
			cooldown_until = NULL, is_action_pending = false, consecutive_failures = 0" > /dev/null
}

# usage: queue_repack TENANT TARGET
queue_repack() {
	sql postgres "INSERT INTO postgresql_maintenance_targets (tenant_key, snapshot_key, target, is_action_pending)
		SELECT tenant_key, 0, $2, true FROM tenants WHERE name = '$1'
		ON CONFLICT (tenant_key, snapshot_key, target) DO UPDATE SET is_action_pending = true, cooldown_until = NULL, consecutive_failures = 0" > /dev/null
}

# usage: last_result TENANT TARGET SNAPSHOT_KEY SINCE
last_result() {
	printf "SELECT coalesce((SELECT last_action_result::text FROM postgresql_maintenance_targets WHERE tenant_key = (SELECT tenant_key FROM tenants WHERE name = '%s') AND target = %s AND snapshot_key = %s AND last_action_at > '%s'), 'none')" "$1" "$2" "$3" "$4"
}

# usage: run_phase TENANT TARGET
run_phase() {
	printf "SELECT coalesce((SELECT phase::text FROM postgresql_maintenance_repack_runs WHERE tenant_key = (SELECT tenant_key FROM tenants WHERE name = '%s') AND target = %s), 'none')" "$1" "$2"
}

# usage: indexes_consistent DATABASE TABLE
# Checks every B-tree index of TABLE against its heap, every heap tuple
# indexed. bt_index_check returns void and raises on an inconsistency.
indexes_consistent() {
	sql "$1" "SELECT bool_and(bt_index_check(i.indexrelid, true)::text = '')
		FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid JOIN pg_am a ON a.oid = c.relam
		WHERE i.indrelid = '$2'::regclass AND a.amname = 'btree'"
}

# usage: guard_locks DATABASE TABLE
# The granted SHARE UPDATE EXCLUSIVE locks on TABLE.
guard_locks() {
	sql "$1" "SELECT count(*) FROM pg_locks WHERE locktype = 'relation' AND relation = '$2'::regclass AND mode = 'ShareUpdateExclusiveLock' AND granted"
}

leftovers="SELECT (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'repack' AND c.relkind = 'r') + (SELECT count(*) FROM pg_trigger WHERE tgname = 'repack_trigger')"

docker network create "$network" > /dev/null
start_postgres "$database"
start_broker "$broker" "postgres://postgres:check@$database:5432/postgres"

expect "create the subscription" 201 "$(api "$broker" "$tenant" POST /subscriptions \
	'{"id":"urn:ngsi-ld:Subscription:repack","type":"Subscription","entities":[{"type":"Probe"}],"notification":{"endpoint":{"uri":"http://127.0.0.1:9/notify","accept":"application/json"}}}')"
expect "create the bloat subscription" 201 "$(api "$broker" "$tenant" POST /subscriptions \
	'{"id":"urn:ngsi-ld:Subscription:bloat","type":"Subscription","entities":[{"type":"Unmatched"}],"notification":{"endpoint":{"uri":"http://127.0.0.1:9/notify","accept":"application/json"}}}')"
expect "create the entity" 201 "$(api "$broker" "$tenant" POST /entities '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":{"type":"Property","value":1}}')"
db="$(tenant_database "$tenant")"
wait_for "the write queued its notification event" t 60 "$db" "SELECT count(*) > 0 FROM notification_events"
wait_for "pg_repack is installed in the tenant database" t 60 "$db" "SELECT count(*) = 1 FROM pg_extension WHERE extname = 'pg_repack'"
sql "$db" "CREATE EXTENSION IF NOT EXISTS amcheck" > /dev/null
targets="'current_attributes', 'entities', 'subscription_delivery_state', 'notification_events', 'notification_deliveries', 'pending_side_effects', 'temporal_append_intents', 'snapshot_notification_outbox', 'entity_search', 'temporal_attribute_streams'"

# 11. Every target table is accepted and survives a swap. The extension's own
# command line repacks each once: it drives the same extension functions, so
# what the extension refuses or loses for a table shows here.
expect "the extension generates a repack for every target" 10 "$(sql "$db" "SELECT count(*) FROM repack.tables WHERE relid IN (SELECT ('public.' || t)::regclass FROM unnest(ARRAY[$targets]) t)")"
expect "no target carries a trigger the run would conflict with" 0 "$(sql "$db" "SELECT count(*) FROM unnest(ARRAY[$targets]) t, repack.conflicted_triggers(('public.' || t)::regclass::oid)")"
constraints="SELECT count(*) FILTER (WHERE convalidated) || ' ' || count(*) FROM pg_constraint WHERE conrelid IN (SELECT ('public.' || t)::regclass FROM unnest(ARRAY[$targets]) t) OR confrelid IN (SELECT ('public.' || t)::regclass FROM unnest(ARRAY[$targets]) t)"
before_constraints="$(sql "$db" "$constraints")"
for target in current_attributes entities subscription_delivery_state notification_events notification_deliveries pending_side_effects temporal_append_intents snapshot_notification_outbox entity_search temporal_attribute_streams; do
	if docker exec -u postgres "$database" pg_repack --no-kill-backend --wait-timeout=60 -d "$db" -t "public.$target" > /dev/null 2>&1; then
		pass "$target is repacked by the extension"
	else
		fail "$target is refused by the extension"
	fi
	expect_true "$target's indexes are consistent after its swap" "$(indexes_consistent "$db" "public.$target")"
done
expect "every constraint on and into the targets is kept" "$before_constraints" "$(sql "$db" "$constraints")"
expect "an entity is created after the swaps" 201 "$(api "$broker" "$tenant" POST /entities '{"id":"urn:ngsi-ld:Probe:after-swap","type":"Probe","temperature":{"type":"Property","value":2}}')"
expect "its key comes from the identity sequence past every earlier key" t "$(sql "$db" "SELECT max(entity_key) = (SELECT entity_key FROM entities WHERE id = 'urn:ngsi-ld:Probe:after-swap') FROM entities")"
expect "an entity referenced by its attributes is deleted after the swaps" 204 "$(api "$broker" "$tenant" DELETE /entities/urn:ngsi-ld:Probe:after-swap)"
expect "its attributes went with it" 0 "$(sql "$db" "SELECT count(*) FROM current_attributes ca WHERE NOT EXISTS (SELECT 1 FROM entities e WHERE e.entity_key = ca.entity_key)")"
expect "no repack object is left by the command line" 0 "$(sql "$db" "$leftovers")"

# 1. A repack under a concurrent writer.
bloat_deliveries "$db" 400000
before="$(sql "$db" "SELECT pg_total_relation_size('notification_deliveries')")"
keep_rows "$db"
since="$(sql postgres 'SELECT now()')"
sql "$db" "DO \$\$
	DECLARE event bigint; subscription bigint; delivery bigint;
	BEGIN
		SELECT subscription_key INTO subscription FROM subscriptions WHERE id = 'urn:ngsi-ld:Subscription:bloat';
		FOR event IN
			SELECT e.event_key FROM notification_events e
			WHERE NOT EXISTS (SELECT 1 FROM notification_deliveries d WHERE d.event_key = e.event_key AND d.subscription_key = subscription)
			ORDER BY e.event_key DESC LIMIT 3000
		LOOP
			INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at, last_error)
			VALUES (event, subscription, now() + interval '10 years', 'writer') RETURNING delivery_key INTO delivery;
			UPDATE notification_deliveries SET attempt_count = attempt_count + 1 WHERE delivery_key = delivery;
			COMMIT;
			PERFORM pg_sleep(0.05);
		END LOOP;
	END \$\$" > /dev/null &
writer=$!
holders+=("$writer")
seed_breach "$tenant" 50 0
wait_for "the repack of notification_deliveries succeeds" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"
wait "$writer" || fail "the concurrent writer failed"
after="$(sql "$db" "SELECT pg_total_relation_size('notification_deliveries')")"
expect_true "the relation shrank by at least half ($before to $after bytes)" "$(sql postgres "SELECT $after * 2 <= $before")"
expect "the rows held before the run are unchanged (gone, new)" "0 0" "$(rows_changed "$db")"
expect "every writer row is present and updated" "3000 3000" "$(sql "$db" "SELECT count(*) || ' ' || sum(attempt_count) FROM notification_deliveries WHERE last_error = 'writer'")"
expect "no repack object is left" 0 "$(sql "$db" "$leftovers")"
expect_true "the repacked indexes are consistent with the heap" "$(indexes_consistent "$db" notification_deliveries)"
expect_true "the target row records the bytes before and after" \
	"$(sql postgres "SELECT last_before_bytes > last_after_bytes FROM postgresql_maintenance_targets WHERE target = 50 AND snapshot_key = 0 AND tenant_key = $(tenant_key "$tenant")")"

# 2. A broker killed in the middle of a run.
bloat_deliveries "$db" 200000
keep_rows "$db"
open_holder "$db" "BEGIN; SELECT 1;"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the run waits in its replay for the older transaction" 4 900 postgres "$(run_phase "$tenant" 50)"
# 12. The guard holds its lock from the setup to the swap.
expect "the guard holds the table's SHARE UPDATE EXCLUSIVE lock during the replay" 1 "$(guard_locks "$db" notification_deliveries)"
docker kill "$broker" > /dev/null
close_holder "COMMIT;"
expect_true "the killed run left its objects" "$(sql "$db" "SELECT ($leftovers) > 0")"
docker start "$broker" > /dev/null
wait_healthy "$broker"
wait_for "the next broker drops the objects" 0 900 "$db" "$leftovers"
wait_for "the run row is deleted" none 60 postgres "$(run_phase "$tenant" 50)"
wait_for "the recovery is recorded" 7 60 postgres "$(last_result "$tenant" 50 0 "$since")"
expect "the table is intact (gone, new)" "0 0" "$(rows_changed "$db")"

# 3. A writer holding the table through the swap.
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
first_fd="$holder_fd"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
open_holder "$db" "BEGIN; SET application_name = 'repack-check-writer'; LOCK TABLE notification_deliveries IN ROW EXCLUSIVE MODE;"
writer_fd="$holder_fd"
holder_fd="$first_fd"
close_holder "COMMIT;"
wait_for "the run gives up waiting for its lock" 4 900 postgres "$(last_result "$tenant" 50 0 "$since")"
expect "the writer holding the table is still alive" 1 "$(sql "$db" "SELECT count(*) FROM pg_stat_activity WHERE application_name = 'repack-check-writer'")"
expect_true "the failure backs off" "$(sql postgres "SELECT cooldown_until > now() AND consecutive_failures > 0 FROM postgresql_maintenance_targets WHERE target = 50 AND snapshot_key = 0 AND tenant_key = $(tenant_key "$tenant")")"
holder_fd="$writer_fd"
close_holder "COMMIT;"
wait_for "the objects are dropped once the writer is gone" 0 900 "$db" "$leftovers"

# 5. A heap of a published Snapshot branch. Forty thousand entities of twelve
# attributes put current_attributes, live and in the branch, past the
# responsive profile's 64 MiB size floor.
for batch in $(seq 0 39); do
	body="$(python3 -c "import json,sys; b=int(sys.argv[1]); print(json.dumps([{'id': f'urn:ngsi-ld:Probe:{b}-{i}', 'type': 'Probe', **{f'p{k}': {'type': 'Property', 'value': 'v' * 40} for k in range(12)}} for i in range(1000)]))" "$batch")"
	api "$broker" "$tenant" POST /entityOperations/create "$body" > /dev/null
done
expect "create the snapshot" 201 "$(api "$broker" "$tenant" POST /snapshots '{"type":"Snapshot","snapshotQueries":[{"type":"Query","entities":[{"type":"Probe"}]}],"snapshotLifetime":"P1D"}')"
wait_for "the snapshot is prepared" t 300 "$db" "SELECT count(*) = 1 FROM snapshot_preparation WHERE status <> 1"
snapshot="$(sql "$db" "SELECT max(snapshot_key) FROM snapshots")"
sql "$db" "DELETE FROM snap_$snapshot.current_attributes WHERE entity_key % 10 <> 0" > /dev/null
sql "$db" "VACUUM snap_$snapshot.current_attributes" > /dev/null
before="$(sql "$db" "SELECT pg_total_relation_size('snap_$snapshot.current_attributes')")"
since="$(sql postgres 'SELECT now()')"
seed_breach "$tenant" 46 "$snapshot"
wait_for "the snapshot branch's current_attributes is repacked" 1 900 postgres "$(last_result "$tenant" 46 "$snapshot" "$since")"
expect_true "the branch relation shrank" "$(sql "$db" "SELECT pg_total_relation_size('snap_$snapshot.current_attributes') < $before")"

# 6. current_attributes written back in key order.
ids="$(sql "$db" "SELECT string_agg(entity_key::text, ',') FROM (SELECT entity_key FROM entities ORDER BY random() LIMIT 100) e")"
buffers() {
	sql "$db" "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT * FROM current_attributes WHERE entity_key IN ($ids)" |
		python3 -c "import json,sys; p=json.load(sys.stdin)[0]['Plan']; print(p['Shared Hit Blocks'] + p['Shared Read Blocks'])"
}
before_buffers="$(buffers)"
since="$(sql postgres 'SELECT now()')"
seed_breach "$tenant" 46 0
sql "$db" "DELETE FROM current_attributes WHERE attribute_name_key % 2 = 0" > /dev/null
sql "$db" "VACUUM current_attributes" > /dev/null
wait_for "the live current_attributes is repacked" 1 900 postgres "$(last_result "$tenant" 46 0 "$since")"
expect_true "the repacked current_attributes indexes are consistent with the heap" "$(indexes_consistent "$db" current_attributes)"
expect_true "entity_key correlates at 0.99 or more" "$(sql "$db" "SELECT correlation >= 0.99 FROM pg_stats WHERE schemaname = 'public' AND tablename = 'current_attributes' AND attname = 'entity_key'")"
after_buffers="$(buffers)"
expect_true "fetching a hundred entities reads fewer buffers ($before_buffers to $after_buffers)" "$(sql postgres "SELECT $after_buffers < $before_buffers")"

# 7. An ALTER TABLE waiting for the table.
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
expect "the guard holds its lock while the ALTER is issued" 1 "$(guard_locks "$db" notification_deliveries)"
started="$(date +%s)"
sql "$db" "SET lock_timeout = 0; ALTER TABLE notification_deliveries ADD COLUMN probe integer DEFAULT 0" > /dev/null &
alter=$!
wait_for "the run yields to the ALTER" 6 60 postgres "$(last_result "$tenant" 50 0 "$since")"
expect_true "the run yields within ten seconds" "$(sql postgres "SELECT $(date +%s) - $started <= 15")"
wait "$alter" && pass "the ALTER completes" || fail "the ALTER failed"
close_holder "COMMIT;"
wait_for "no repack object is left" 0 60 "$db" "$leftovers"
sql "$db" "ALTER TABLE notification_deliveries DROP COLUMN probe" > /dev/null

# 13. A CREATE INDEX waiting for the table. Without the guard's lock it would
# build on the old files, and the swap would leave it pointing at them.
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
sql "$db" "SET lock_timeout = 0; CREATE INDEX repack_check_probe ON notification_deliveries (attempt_count, delivery_key)" > /dev/null &
build=$!
wait_for "the run yields to the CREATE INDEX" 6 60 postgres "$(last_result "$tenant" 50 0 "$since")"
wait "$build" && pass "the CREATE INDEX completes" || fail "the CREATE INDEX failed"
close_holder "COMMIT;"
wait_for "no repack object is left" 0 60 "$db" "$leftovers"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the next repack, with the new index, succeeds" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"
expect_true "every index, the new one too, is consistent with the heap" "$(indexes_consistent "$db" notification_deliveries)"
sql "$db" "DROP INDEX repack_check_probe" > /dev/null

# 14. A broker stopped in the middle of a run cleans up within its budget.
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
started="$(date +%s)"
docker stop -t 90 "$broker" > /dev/null
stopped="$(date +%s)"
expect_true "the broker stops within its 30-second cleanup budget and some margin ($((stopped - started)) s)" "$(sql postgres "SELECT $stopped - $started <= 40")"
expect "the stopped run cleaned up its objects" 0 "$(sql "$db" "$leftovers")"
expect "the stopped run deleted its row" none "$(sql postgres "$(run_phase "$tenant" 50)")"
close_holder "COMMIT;"
since="$(sql postgres 'SELECT now()')"
docker start "$broker" > /dev/null
wait_healthy "$broker"
wait_for "the stopped repack, still queued, runs after the restart" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"

# 16. A bulk insert committed while the swap waits for its lock.
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
old_fd="$holder_fd"
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
open_holder "$db" "BEGIN; INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at, last_error)
	SELECT e.event_key, (SELECT subscription_key FROM subscriptions WHERE id = 'urn:ngsi-ld:Subscription:bloat'), now() + interval '10 years', 'bulk' FROM notification_events e
	WHERE NOT EXISTS (SELECT 1 FROM notification_deliveries d WHERE d.event_key = e.event_key) LIMIT 100000;"
bulk_fd="$holder_fd"
holder_fd="$old_fd"
close_holder "COMMIT;"
wait_for "the run waits for its swap's lock" 5 120 postgres "$(run_phase "$tenant" 50)"
latencies="$(mktemp)"
sql_steps "$db" "CREATE TEMP TABLE waits (millis double precision)" "DO \$\$
	DECLARE started timestamptz;
	BEGIN
		FOR i IN 1..1200 LOOP
			started := clock_timestamp();
			UPDATE notification_deliveries SET attempt_count = attempt_count WHERE delivery_key = (SELECT min(delivery_key) FROM notification_deliveries WHERE last_error = repeat('x', 300));
			COMMIT;
			INSERT INTO waits VALUES (extract(epoch FROM clock_timestamp() - started) * 1000);
			PERFORM pg_sleep(0.05);
		END LOOP;
	END \$\$" "SELECT round(max(millis)) FROM waits" > "$latencies" &
probe=$!
sleep 5
holder_fd="$bulk_fd"
close_holder "COMMIT;"
wait_for "the repack after the bulk insert succeeds" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"
wait "$probe" || fail "the concurrent writer failed"
expect_true "the bulk insert is in the table" "$(sql "$db" "SELECT count(*) = 100000 FROM notification_deliveries WHERE last_error = 'bulk'")"
expect_true "no write waited two seconds behind the swap ($(cat "$latencies") ms)" "$(sql postgres "SELECT $(cat "$latencies") < 2000")"
rm -f "$latencies"

# 8. Another tenant's queued analyze while a repack is held open.
expect "create the other tenant's entity" 201 "$(api "$broker" "$other" POST /entities '{"id":"urn:ngsi-ld:Probe:2","type":"Probe","temperature":{"type":"Property","value":1}}')"
other_db="$(tenant_database "$other")"
sql "$other_db" "ALTER TABLE entities SET (autovacuum_enabled = off)" > /dev/null
sql "$other_db" "DO \$\$ BEGIN FOR i IN 1..60000 LOOP UPDATE entities SET write_count = write_count; END LOOP; END \$\$" > /dev/null
bloat_deliveries "$db" 200000
open_holder "$db" "BEGIN; SELECT 1;"
queue_repack "$tenant" 50
wait_for "the run waits in its replay" 4 900 postgres "$(run_phase "$tenant" 50)"
since="$(sql postgres 'SELECT now()')"
sql postgres "INSERT INTO postgresql_maintenance_targets (tenant_key, snapshot_key, target, is_action_pending)
	SELECT tenant_key, 0, 23, true FROM tenants WHERE name = '$other'
	ON CONFLICT (tenant_key, snapshot_key, target) DO UPDATE SET is_action_pending = true, cooldown_until = NULL" > /dev/null
wait_for "the other tenant's analyze runs while the repack is held" 1 900 postgres "$(last_result "$other" 23 0 "$since")"
expect "the repack is still held" 4 "$(sql postgres "$(run_phase "$tenant" 50)")"
close_holder "COMMIT;"
wait_for "the held repack finishes" none 900 postgres "$(run_phase "$tenant" 50)"

# 15. A queued repack held back by a gate leaves the next one its turn. The
# held-back tenant is due first: the task goes through tenants in key order.
expect_true "the held-back tenant is due before the other" "$(sql postgres "SELECT $(tenant_key "$tenant") < $(tenant_key "$other")")"
expect "create the other tenant's subscription" 201 "$(api "$broker" "$other" POST /subscriptions \
	'{"id":"urn:ngsi-ld:Subscription:repack","type":"Subscription","entities":[{"type":"Probe"}],"notification":{"endpoint":{"uri":"http://127.0.0.1:9/notify","accept":"application/json"}}}')"
expect "create the other tenant's bloat subscription" 201 "$(api "$broker" "$other" POST /subscriptions \
	'{"id":"urn:ngsi-ld:Subscription:bloat","type":"Subscription","entities":[{"type":"Unmatched"}],"notification":{"endpoint":{"uri":"http://127.0.0.1:9/notify","accept":"application/json"}}}')"
expect "write the other tenant's entity" 204 "$(api "$broker" "$other" PATCH /entities/urn:ngsi-ld:Probe:2/attrs '{"temperature":{"type":"Property","value":3}}')"
wait_for "the other tenant's write queued its notification event" t 60 "$other_db" "SELECT count(*) > 0 FROM notification_events"
bloat_deliveries "$db" 200000
bloat_deliveries "$other_db" 200000
open_holder "$db" "BEGIN; SELECT txid_current();"
sleep 35
since="$(sql postgres 'SELECT now()')"
queue_repack "$tenant" 50
queue_repack "$other" 50
wait_for "the other tenant's repack runs while the first is held back" 1 900 postgres "$(last_result "$other" 50 0 "$since")"
expect "the held-back repack is still queued" t "$(sql postgres "SELECT is_action_pending FROM postgresql_maintenance_targets WHERE target = 50 AND snapshot_key = 0 AND tenant_key = $(tenant_key "$tenant")")"
close_holder "COMMIT;"
wait_for "the held-back repack runs once its gate lifts" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"

# 10. Write latency on current_attributes during a repack. Rewriting every row
# twice triples the heap the last repack compacted, past the size floor, and
# vacuum leaves two thirds of it free.
sql "$db" "UPDATE current_attributes SET modified_at = modified_at" > /dev/null
sql "$db" "UPDATE current_attributes SET modified_at = modified_at" > /dev/null
sql "$db" "VACUUM current_attributes" > /dev/null
since="$(sql postgres 'SELECT now()')"
seed_breach "$tenant" 46 0
sql postgres "UPDATE postgresql_maintenance_targets SET cooldown_until = NULL WHERE target = 46 AND snapshot_key = 0" > /dev/null
latency="$(sql_steps "$db" "CREATE TEMP TABLE timings (at timestamptz, millis double precision)" "DO \$\$
	DECLARE started timestamptz; key bigint;
	BEGIN
		FOR i IN 1..6000 LOOP
			SELECT entity_key INTO key FROM entities ORDER BY random() LIMIT 1;
			started := clock_timestamp();
			UPDATE current_attributes SET modified_at = modified_at WHERE entity_key = key;
			COMMIT;
			INSERT INTO timings VALUES (started, extract(epoch FROM clock_timestamp() - started) * 1000);
			PERFORM pg_sleep(0.02);
		END LOOP;
	END \$\$" "SELECT percentile_cont(0.99) WITHIN GROUP (ORDER BY millis) FILTER (WHERE at < (SELECT min(at) FROM timings) + interval '20 seconds')
		|| ' ' || percentile_cont(0.99) WITHIN GROUP (ORDER BY millis) FILTER (WHERE at >= (SELECT min(at) FROM timings) + interval '20 seconds')
	FROM timings")"
read -r baseline_p99 run_p99 <<< "$latency"
wait_for "the repack under write load succeeds" 1 900 postgres "$(last_result "$tenant" 46 0 "$since")"
expect_true "p99 write latency stays within 250 ms of its baseline ($baseline_p99 to $run_p99 ms)" "$(sql postgres "SELECT $run_p99 <= $baseline_p99 + 250")"

# 9. A tenant whose pg_repack is gone and unrecorded.
docker stop "$broker" > /dev/null
sql "$db" "DROP EXTENSION pg_repack" > /dev/null
sql postgres "DELETE FROM tenant_extension_versions WHERE extension = 'pg_repack' AND tenant_key = $(tenant_key "$tenant")" > /dev/null
docker start "$broker" > /dev/null
wait_healthy "$broker"
wait_for "the wake catch-up installs pg_repack again" 1 600 "$db" "SELECT count(*) FROM pg_extension WHERE extname = 'pg_repack'"

# 4. A role that may not create pg_repack. The tenant's tables pass to a role
# that is no superuser, which then runs the broker.
docker stop "$broker" > /dev/null
sql postgres "CREATE ROLE $role LOGIN PASSWORD 'check'; GRANT pg_stat_scan_tables, pg_read_all_stats TO $role" > /dev/null
grant_role postgres
sql "$db" "DROP EXTENSION pg_repack" > /dev/null
sql "$db" "DO \$\$ DECLARE t record; BEGIN FOR t IN SELECT tablename FROM pg_tables WHERE schemaname = 'public' LOOP EXECUTE format('ALTER TABLE public.%I OWNER TO $role', t.tablename); END LOOP; END \$\$" > /dev/null
grant_role "$db"
grant_role "$other_db"
start_broker "$owner_broker" "postgres://$role:check@$database:5432/postgres"
expect "the role may not create pg_repack" 0 "$(sql "$db" "SELECT count(*) FROM pg_extension WHERE extname = 'pg_repack'")"
bloat_deliveries "$db" 200000
seed_breach "$tenant" 50 0
for _ in $(seq 1 900); do
	grep -q 'missing-pg-repack' <<< "$(docker logs "$owner_broker" 2>&1)" && break
	sleep 1
done
if grep -q 'missing-pg-repack' <<< "$(docker logs "$owner_broker" 2>&1)"; then
	pass "the missing extension is reported"
else
	fail "the missing extension is not reported"
fi
wait_for "without the extension the repack is only reported" 3 900 postgres \
	"SELECT coalesce(last_decision::text, 'none') FROM postgresql_maintenance_targets WHERE target = 50 AND snapshot_key = 0 AND tenant_key = $(tenant_key "$tenant")"
since="$(sql postgres 'SELECT now()')"
sql "$db" "CREATE EXTENSION pg_repack; GRANT USAGE, CREATE ON SCHEMA repack TO $role" > /dev/null
seed_breach "$tenant" 50 0
for _ in $(seq 1 900); do
	grep -q 'missing-repack-right' <<< "$(docker logs "$owner_broker" 2>&1)" && break
	sleep 1
done
if grep -q 'missing-repack-right' <<< "$(docker logs "$owner_broker" 2>&1)"; then
	pass "the schema alone, without the repack.tables view, is reported as a missing right"
else
	fail "the missing right on repack.tables is not reported"
fi
since="$(sql postgres 'SELECT now()')"
sql "$db" "GRANT SELECT ON repack.tables TO $role" > /dev/null
seed_breach "$tenant" 50 0
wait_for "with the extension, the schema and the view granted, the owner's repack runs" 1 900 postgres "$(last_result "$tenant" 50 0 "$since")"

for container in "$broker" "$owner_broker"; do
	logs="$(docker logs "$container" 2>&1)"
	if grep -qE 'ERROR' <<< "$logs"; then
		fail "broker log of $container carries an error"
		grep -E 'ERROR' <<< "$logs" >&2
	else
		pass "broker log of $container carries no error"
	fi
done

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
