#!/usr/bin/env bash
# usage: columnstore-conversion-check.sh IMAGE BROKER_IMAGE
# Asserts that history conversion depends on no TimescaleDB background worker:
# on a server allowed one background worker, which its launcher takes, two
# tenants each convert a history chunk past the horizon on the broker's own
# sweep, install no TimescaleDB job, and keep every row readable. IMAGE is the
# TSL edition, which has a columnstore.
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
	docker exec -u postgres "$database" psql -XAtq -v ON_ERROR_STOP=1 -d "$1" -c "$2"
}

# usage: tenant_database TENANT
tenant_database() {
	sql postgres "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${1}%'"
}

# The aged chunks of attribute_history, as "<count> <all converted>".
aged_chunks="SELECT count(*) || ' ' || coalesce(bool_and(is_compressed), false) FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end <= now() - interval '30 days'"

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
expect "timescaledb.max_background_workers" 1 "$(sql postgres 'SHOW timescaledb.max_background_workers')"

docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
	-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
	-e "ANDROMEDA__DATABASE__URL=postgres://postgres:check@$database:5432/postgres" \
	-e ANDROMEDA__TEMPORAL__COLUMNSTORE_SWEEP_INTERVAL=2s \
	"$broker_image" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
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
	# History is partitioned on the write clock, so a row recorded sixty days
	# ago lands in a chunk past the thirty-day horizon.
	sql "$db" "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
		UPDATE aged SET recorded_at = now() - interval '60 days', write_id = gen_random_uuid(),
			generated_instance_id = CASE WHEN generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END;
		INSERT INTO attribute_history SELECT * FROM aged;" > /dev/null
done

expect "TimescaleDB schedulers running" 0 \
	"$(sql postgres "SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'TimescaleDB Background Worker Scheduler'")"

for tenant in "${tenants[@]}"; do
	db="$(tenant_database "$tenant")"
	converted=''
	for _ in $(seq 1 60); do
		converted="$(sql "$db" "$aged_chunks")"
		[ "$converted" = '1 true' ] && break
		sleep 0.5
	done
	expect "$tenant: the aged chunk is converted by the sweep" '1 true' "$converted"
	expect "$tenant: the current chunk stays in the rowstore" f \
		"$(sql "$db" "SELECT bool_or(is_compressed) FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end > now()")"
	expect "$tenant: TimescaleDB jobs on attribute_history" 0 \
		"$(sql "$db" "SELECT count(*) FROM timescaledb_information.jobs WHERE hypertable_name = 'attribute_history'")"
	expect "$tenant: rows readable after conversion" 2 "$(sql "$db" 'SELECT count(*) FROM attribute_history')"
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
