#!/usr/bin/env bash
# usage: recompression-check.sh IMAGE BROKER_IMAGE
# Asserts that the maintenance controller repairs a partially converted
# history chunk: once the broker's sweep has converted an aged chunk (status
# 1), an UPDATE into it leaves it partial (status 9), and the controller in
# automatic mode recompresses it back to status 1. Also asserts that the fleet
# observation reports a tenant-shaped database no registry row names. IMAGE is
# the TSL edition, which has a columnstore.
#
# The controller runs the responsive profile, so the chunk qualifies after a
# breach run of three minutes and is recompressed on the tenant's next visit
# for its queued action, five minutes later; the check waits up to fifteen.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="recompression-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
tenant=rca
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
sql postgres "CREATE DATABASE andromeda_tenant_orphan_0badc0de" > /dev/null

docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
	-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
	-e "ANDROMEDA__DATABASE__URL=postgres://postgres:check@$database:5432/postgres" \
	-e ANDROMEDA__DATABASE__MAINTENANCE__MODE=automatic \
	-e ANDROMEDA__DATABASE__MAINTENANCE__PROFILE=responsive \
	-e ANDROMEDA__TEMPORAL__COLUMNSTORE_SWEEP_INTERVAL=2s \
	"$broker_image" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
		break
	fi
	sleep 0.5
done

status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
	--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":[{"type":"Property","value":1,"observedAt":"2026-10-17T10:00:00Z"}]}' \
	"$base/temporal/entities")"
expect "create temporal entity" 201 "$status"
db="$(sql postgres "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'")"
# History is partitioned on the write clock, so a row recorded sixty days ago
# lands in a chunk past the thirty-day horizon.
sql "$db" "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
	UPDATE aged SET recorded_at = now() - interval '60 days', write_id = gen_random_uuid(),
		generated_instance_id = CASE WHEN generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END;
	INSERT INTO attribute_history SELECT * FROM aged;" > /dev/null

wait_for "the sweep converts the aged chunk" 1 60 "$db" "$aged_status"
sql "$db" "UPDATE attribute_history SET modified_at = modified_at WHERE recorded_at <= now() - interval '30 days'" > /dev/null
expect "an UPDATE into the converted chunk leaves it partial" 9 "$(sql "$db" "$aged_status")"
wait_for "the controller recompresses the partial chunk" 1 900 "$db" "$aged_status"
expect "rows readable after recompression" 2 "$(sql "$db" 'SELECT count(*) FROM attribute_history')"

# Target 35 is attribute_history_columnstore; result 1 is succeeded.
wait_for "the platform records the recompression as succeeded" 1 30 postgres \
	"SELECT coalesce(string_agg(last_action_result::text, ','), 'none') FROM postgresql_maintenance_targets WHERE target = 35 AND snapshot_key IS NULL"

logs="$(docker logs "$broker" 2>&1)"
if grep -q 'orphan-tenant-database' <<< "$logs" && grep -q 'andromeda_tenant_orphan_0badc0de' <<< "$logs"; then
	pass "the fleet observation reports the unregistered tenant-shaped database"
else
	fail "the fleet observation does not report the unregistered tenant-shaped database"
fi
if grep -qE 'columnstore-overdue|timescaledb-job-unexpected' <<< "$logs"; then
	fail "the broker log reports an overdue chunk or an unexpected job"
else
	pass "no overdue chunk and no unexpected job reported"
fi
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
