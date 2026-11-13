#!/usr/bin/env bash
# usage: columnstore-conversion-check.sh IMAGE BROKER_IMAGE
# Asserts that history conversion depends on no TimescaleDB background worker:
# on a server allowed one background worker, which its launcher takes, two
# tenants each convert a history chunk past the horizon on the broker's own
# sweep, truncating its rowstore, keep a chunk holding too few rows per stream
# in the rowstore, install no TimescaleDB job, and keep every row readable.
# IMAGE is the TSL edition, which has a columnstore.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="columnstore-conversion-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
tenants=(cva cvb)
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

# usage: sql DATABASE QUERY
sql() {
	docker exec -u postgres "$database" psql -U andromeda -XAtq -v ON_ERROR_STOP=1 -d "$1" -c "$2"
}

# usage: tenant_database TENANT
tenant_database() {
	sql andromeda "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${1}%'"
}

# The aged chunks of attribute_history, as "<converted> <in the rowstore>".
aged_chunks="SELECT count(*) FILTER (WHERE is_compressed) || ' ' || count(*) FILTER (WHERE NOT is_compressed) FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end <= now() - interval '30 days'"
# The main-fork bytes of every converted chunk's rowstore, summed.
converted_rowstore_bytes="SELECT coalesce(sum(pg_relation_size(format('%I.%I', chunk_schema, chunk_name)::regclass, 'main')), -1) FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND is_compressed"

docker network create "$network" > /dev/null
# timescaledb-tune refuses fewer than 16 background workers, so the limit is
# given to the server directly.
docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$image" -c timescaledb.max_background_workers=1 > /dev/null
for _ in $(seq 1 120); do
	if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
		break
	fi
	sleep 0.5
done
expect "timescaledb.max_background_workers" 1 "$(sql andromeda 'SHOW timescaledb.max_background_workers')"

docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
	-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
	-e "ANDROMEDA__DATABASE__CONNECTION=postgres://andromeda:check@$database:5432/andromeda" \
	-e ANDROMEDA__TEMPORAL__STORAGE__COLUMNSTORE__SWEEP_INTERVAL=2s \
	"$broker_image" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if docker exec "$broker" andromeda health > /dev/null 2>&1; then
		break
	fi
	sleep 0.5
done

for tenant in "${tenants[@]}"; do
	status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
		--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":[{"type":"Property","value":1,"observedAt":"2026-10-17T10:00:00Z"}]}' \
		"$base/temporal/entities")"
	expect "$tenant: create temporal entity" 201 "$status"
	db="$(tenant_database "$tenant")"
	# History is partitioned on the write clock, so rows recorded sixty and
	# ninety days ago land in chunks past the thirty-day horizon: eight
	# instances of the one stream in the first, which pays to convert, and one
	# in the second, which does not.
	sql "$db" "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
		INSERT INTO attribute_history SELECT
			now() - interval '60 days' + n * interval '1 minute', gen_random_uuid(), a.stream_key, a.write_ordinal, a.attribute_kind, a.value_kind,
			a.observed_at, a.created_at, a.modified_at, a.deleted_at, a.value_date_time, a.expires_at,
			CASE WHEN a.generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END,
			a.value_type_key, a.value_datatype_key, a.unit_code_id, a.value_boolean, a.verbatim_instance_id, a.value_number, a.value_string,
			a.value_language, a.value_geometry, a.value_geometry_valid, a.value_geometry_reach, a.value_geometry_members, a.value_object, a.value_uri,
			a.object_type_keys, a.proof_seals, a.sub_attributes
		FROM aged a CROSS JOIN generate_series(1, 8) n;
		UPDATE aged SET recorded_at = now() - interval '90 days', write_id = gen_random_uuid(),
			generated_instance_id = CASE WHEN generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END;
		INSERT INTO attribute_history SELECT * FROM aged;" > /dev/null
done

expect "TimescaleDB schedulers running" 0 \
	"$(sql andromeda "SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'TimescaleDB Background Worker Scheduler'")"

for tenant in "${tenants[@]}"; do
	db="$(tenant_database "$tenant")"
	converted=''
	kept=''
	for _ in $(seq 1 60); do
		converted="$(sql "$db" "$aged_chunks")"
		kept="$(sql "$db" 'SELECT count(*) FROM rowstore_history_chunks')"
		[ "$converted" = '1 1' ] && [ "$kept" = 1 ] && break
		sleep 0.5
	done
	expect "$tenant: one aged chunk is converted and one stays in the rowstore" '1 1' "$converted"
	expect "$tenant: the sweep records the chunk it keeps" 1 "$kept"
	expect "$tenant: the kept chunk holds one row of one stream" '1 1' \
		"$(sql "$db" "SELECT row_count || ' ' || stream_count FROM rowstore_history_chunks")"
	expect "$tenant: the converted chunk's rowstore is truncated" 0 "$(sql "$db" "$converted_rowstore_bytes")"
	expect "$tenant: the current chunk stays in the rowstore" f \
		"$(sql "$db" "SELECT bool_or(is_compressed) FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end > now()")"
	expect "$tenant: TimescaleDB jobs on attribute_history" 0 \
		"$(sql "$db" "SELECT count(*) FROM timescaledb_information.jobs WHERE hypertable_name = 'attribute_history'")"
	expect "$tenant: rows readable after conversion" 10 "$(sql "$db" 'SELECT count(*) FROM attribute_history')"
done

if docker logs "$broker" 2>&1 | grep -qE 'ERROR'; then
	fail "broker log carries an error"
	docker logs "$broker" 2>&1 | grep -E 'ERROR' >&2
else
	pass "broker log carries no error"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
