#!/usr/bin/env bash
# usage: pgstattuple-absence-check.sh IMAGE BROKER_IMAGE
# Asserts that automatic maintenance of a tenant without pgstattuple runs
# everything but the index probes: it reports autovacuum-overdue, runs the
# catch-up vacuum, recompresses a history chunk an UPDATE left partial,
# probes no B-tree or GiST index, and warns about the missing extension once.
# IMAGE is the TSL edition, which has a columnstore.
#
# The tenant is the default one, whose tables live in the platform database.
# The broker installs pgstattuple when it opens a tenant and keeps the
# default tenant open from startup, so the extension dropped after startup
# stays dropped. Autovacuum is off on current_attributes, so the dead tuples
# of repeated updates pile up until the controller vacuums them.
#
# The controller runs the responsive profile: a heap or chunk qualifies after
# a breach run of three minutes and its action runs on the tenant's next visit
# for queued work; the check waits up to fifteen minutes for each.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="pgstattuple-absence-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
failures=0

cleanup() {
	docker rm -f "$broker" "$database" > /dev/null 2>&1 || :
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

# usage: sql QUERY
sql() {
	docker exec -u postgres "$database" psql -XAtq -v ON_ERROR_STOP=1 -d postgres -c "$1"
}

# usage: wait_for DESCRIPTION EXPECTED SECONDS QUERY
wait_for() {
	local actual=''
	for _ in $(seq 1 "$3"); do
		actual="$(sql "$4")"
		[ "$actual" = "$2" ] && break
		sleep 1
	done
	expect "$1" "$2" "$actual"
}

# usage: create_batch FIRST LAST
create_batch() {
	local body
	body="$(seq "$1" "$2" | awk 'BEGIN { printf "[" } NR > 1 { printf "," } { printf "{\"id\":\"urn:ngsi-ld:Probe:%d\",\"type\":\"Probe\",\"temperature\":{\"type\":\"Property\",\"value\":%d}}", $1, $1 } END { printf "]" }')"
	curl -sS -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
		--data "$body" "$base/entityOperations/create"
}

# The status of the aged chunk of attribute_history: 1 converted, 9 converted
# and partial.
aged_status="SELECT string_agg(c.status::text, ',') FROM timescaledb_information.chunks ch JOIN _timescaledb_catalog.chunk c ON c.relid = format('%I.%I', ch.chunk_schema, ch.chunk_name)::regclass WHERE ch.hypertable_name = 'attribute_history' AND ch.range_end <= now() - interval '30 days'"

docker network create "$network" > /dev/null
docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$image" > /dev/null
for _ in $(seq 1 120); do
	if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
		break
	fi
	sleep 0.5
done

docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
	-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
	-e "ANDROMEDA__DATABASE__URL=postgres://postgres:check@$database:5432/postgres" \
	-e ANDROMEDA__DATABASE__MAINTENANCE__MODE=automatic \
	-e ANDROMEDA__DATABASE__MAINTENANCE__PROFILE=responsive \
	-e ANDROMEDA__TEMPORAL__STORAGE__COLUMNSTORE__SWEEP_INTERVAL=2s \
	"$broker_image" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
		break
	fi
	sleep 0.5
done

sql "DROP EXTENSION pgstattuple; ALTER TABLE current_attributes SET (autovacuum_enabled = false)" > /dev/null
expect "pgstattuple is absent from the default tenant's database" 0 "$(sql "SELECT count(*) FROM pg_extension WHERE extname = 'pgstattuple'")"
key="$(sql 'SELECT tenant_key FROM tenants WHERE is_unnamed_default')"

status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' \
	--data '{"id":"urn:ngsi-ld:Probe:0","type":"Probe","temperature":[{"type":"Property","value":0,"observedAt":"2026-10-17T10:00:00Z"}]}' \
	"$base/temporal/entities")"
expect "create temporal entity" 201 "$status"
for first in $(seq 1 500 39501); do
	created="$(create_batch "$first" "$((first + 499))")"
	if [ "$created" != 201 ]; then
		fail "create entities $first to $((first + 499)): expected '201', got '$created'"
	fi
done

# History is partitioned on the write clock, so rows recorded sixty days ago
# land in a chunk past the thirty-day horizon; eight instances of the one
# stream make it pay to convert rather than stay in the rowstore.
sql "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
	INSERT INTO attribute_history SELECT
		now() - interval '60 days' + n * interval '1 minute', gen_random_uuid(), a.stream_key, a.write_ordinal, a.attribute_kind, a.value_kind,
		a.observed_at, a.created_at, a.modified_at, a.deleted_at, a.value_date_time, a.expires_at,
		CASE WHEN a.generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END,
		a.value_type_key, a.value_datatype_key, a.unit_code_id, a.value_boolean, a.verbatim_instance_id, a.value_number, a.value_string,
		a.value_language, a.value_geometry, a.value_geometry_valid, a.value_geometry_reach, a.value_geometry_members, a.value_object, a.value_uri,
		a.object_type_keys, a.proof_seals, a.sub_attributes
	FROM aged a CROSS JOIN generate_series(1, 8) n;" > /dev/null
wait_for "the sweep converts the aged chunk" 1 60 "$aged_status"
sql "UPDATE attribute_history SET modified_at = modified_at WHERE recorded_at <= now() - interval '30 days'" > /dev/null
expect "an UPDATE into the converted chunk leaves it partial" 9 "$(sql "$aged_status")"

# Each rewrite of every row leaves one more dead tuple per live one: after n
# rewrites the dead share is n/(n+1). The rewrites go on until the relation
# reaches 64 MiB, and for at least four, so the dead share is at least 80%
# and the reclaimable bytes at least 51 MiB, past the responsive profile's
# 10% dead share and its 16 MiB size and reclaim floors whatever the row
# width.
#
# Page pruning reclaims a dead HOT version on the next visit to its page, so
# a session holds the database's xmin horizon back while the rewrites run:
# no version is removable, the pages fill, and every rewrite after that is
# not HOT and adds an entry per row to pk_ca_grain. They go on until that
# B-tree reaches 16 MiB too: an index under the size floor is never a probe
# candidate, and the absence of probes asserted below would then prove
# nothing. A relation or index still short after the cap ends the check here
# rather than after the fifteen-minute wait.
docker exec -d -u postgres "$database" psql -XAtq -d postgres -c "SET application_name = 'horizon-hold'; SELECT pg_sleep(3600)"
for _ in $(seq 1 20); do
	[ "$(sql "SELECT count(*) FROM pg_stat_activity WHERE application_name = 'horizon-hold' AND state = 'active'")" = 1 ] && break
	sleep 0.5
done
heap_floor=$((64 * 1024 * 1024))
index_floor=$((16 * 1024 * 1024))
rewrites=0
heap_bytes=0
index_bytes=0
while [ "$rewrites" -lt 60 ]; do
	sql "UPDATE current_attributes SET modified_at = modified_at" > /dev/null
	rewrites=$((rewrites + 1))
	heap_bytes="$(sql "SELECT pg_total_relation_size('current_attributes')")"
	index_bytes="$(sql "SELECT pg_relation_size('pk_ca_grain')")"
	if [ "$rewrites" -ge 4 ] && [ "$heap_bytes" -ge "$heap_floor" ] && [ "$index_bytes" -ge "$index_floor" ]; then
		break
	fi
done
sql "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'horizon-hold'" > /dev/null
if [ "$heap_bytes" -lt "$heap_floor" ] || [ "$index_bytes" -lt "$index_floor" ]; then
	fail "current_attributes reaches 64 MiB and pk_ca_grain 16 MiB: $heap_bytes and $index_bytes bytes after $rewrites rewrites"
	exit 1
fi
pass "current_attributes reaches 64 MiB and pk_ca_grain 16 MiB: $heap_bytes and $index_bytes bytes after $rewrites rewrites"

# Target 11 is current_attributes and 35 attribute_history_columnstore,
# snapshot key 0 the live branch; result 1 is succeeded.
wait_for "the catch-up vacuum of current_attributes succeeds" 1 900 \
	"SELECT coalesce(string_agg(last_action_result::text, ','), 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 11"
wait_for "the controller recompresses the partial chunk" 1 900 "$aged_status"
wait_for "the platform records the recompression as succeeded" 1 30 \
	"SELECT coalesce(string_agg(last_action_result::text, ','), 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 35"
# The probeable targets: every B-tree and GiST index the controller measures
# with pgstattuple. A GIN is measured by its density and is not among them.
# pk_ca_grain (target 1) is past the size floor and was never probed, so it
# was a candidate every visit left unprobed.
probeable='1, 2, 3, 5, 6, 7, 8, 9, 31, 32, 33, 34, 39, 40, 41, 42, 43'
expect "no B-tree or GiST index records a probe" 0 \
	"$(sql "SELECT count(*) FROM postgresql_maintenance_targets WHERE tenant_key = $key AND target IN ($probeable) AND last_sampled_at IS NOT NULL")"
expect "no B-tree or GiST index records a rebuild or a queued one" 0 \
	"$(sql "SELECT count(*) FROM postgresql_maintenance_targets WHERE tenant_key = $key AND target IN ($probeable) AND (last_action_result IS NOT NULL OR is_action_pending)")"
expect "pgstattuple is still absent" 0 "$(sql "SELECT count(*) FROM pg_extension WHERE extname = 'pgstattuple'")"

logs="$(docker logs "$broker" 2>&1)"
if grep 'autovacuum-overdue' <<< "$logs" | grep -q 'current_attributes'; then
	pass "autovacuum-overdue is reported for current_attributes"
else
	fail "autovacuum-overdue is not reported for current_attributes"
fi
if grep 'action completed' <<< "$logs" | grep 'acted' | grep -q 'current_attributes'; then
	pass "the catch-up vacuum logs acted"
else
	fail "the catch-up vacuum logs no acted decision"
fi
if grep -q 'index probe completed' <<< "$logs"; then
	fail "an index probe ran"
else
	pass "no index probe ran"
fi
# The visits waited for above span many passes over the one tenant, each of
# which reads the capability again.
warnings="$(grep 'capability-unavailable' <<< "$logs" | grep -c 'missing-pgstattuple' || :)"
expect "one capability-unavailable warning names the missing extension" 1 "$warnings"
if grep -qE 'ERROR' <<< "$logs"; then
	fail "broker log carries an error"
	grep -E 'ERROR' <<< "$logs" >&2
else
	pass "broker log carries no error"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
