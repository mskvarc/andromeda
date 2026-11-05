#!/usr/bin/env bash
# usage: history-wake-check.sh IMAGE BROKER_IMAGE [APACHE_IMAGE]
# Asserts that a tenant no request loads still has its history converted and
# aged out. Each case writes eight history rows recorded sixty days ago into
# one tenant, past both the thirty-day columnstore horizon and the thirty-day
# retention age, then restarts the broker and sends no further request: only
# the wake loop can load the tenant again.
#
# - IMAGE (TSL edition), retention enabled: the aged chunk is dropped, and once
#   the woken tenant unloads its next wake is the end of its newest chunk plus
#   the horizon, weeks ahead.
# - IMAGE, retention disabled: the aged chunk is converted to the columnstore
#   and no columnstore-overdue is logged.
# - APACHE_IMAGE, retention disabled, when given: nothing is ever due, so the
#   reconciliation probe records no wake and the chunk stays in place.
#
# Both sweeps run every hour, so neither can act on the aged rows from the
# load the setup request caused; the work a case waits for is done by the load
# the wake loop makes. Each case waits up to five minutes.
set -Eeuo pipefail

image="$1"
broker_image="$2"
apache_image="${3:-}"
prefix="history-wake-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
tenant=hwc
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

# The chunks of attribute_history whose range ended thirty days ago or more,
# with whether each is converted.
aged_chunks="SELECT coalesce(string_agg(is_compressed::text, ','), 'none') FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end <= now() - interval '30 days'"

# Whether the tenant's wake schedule row holds no wake, a wake weeks ahead, or
# one sooner.
wake_state="SELECT coalesce((SELECT CASE WHEN w.wake_at IS NULL THEN 'none' WHEN w.wake_at > now() + interval '7 days' THEN 'weeks-ahead' ELSE 'soon' END FROM tenant_wake_schedule w JOIN tenants t USING (tenant_key) WHERE t.name = '$tenant'), 'unprobed')"

# usage: run_case LABEL IMAGE RETENTION_MODE
# Leaves the case's database and broker running for the assertions that follow.
run_case() {
	cleanup
	docker network create "$network" > /dev/null
	docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$2" > /dev/null
	for _ in $(seq 1 120); do
		if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
			break
		fi
		sleep 0.5
	done

	docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__URL=postgres://andromeda:check@$database:5432/andromeda" \
		-e ANDROMEDA__DATABASE__MAINTENANCE__MODE=observe \
		-e "ANDROMEDA__TEMPORAL__STORAGE__RETENTION__MODE=$3" \
		-e ANDROMEDA__TEMPORAL__STORAGE__RETENTION__DELETE_AFTER=30days \
		-e ANDROMEDA__TENANCY__RESIDENCY__IDLE_EVICTION_AFTER=2m \
		-e ANDROMEDA__TENANCY__RESIDENCY__WOKEN_LIFETIME=30s \
		"$broker_image" > /dev/null
	local base
	base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
			break
		fi
		sleep 0.5
	done

	local status
	status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
		--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":[{"type":"Property","value":1,"observedAt":"2026-10-17T10:00:00Z"}]}' \
		"$base/temporal/entities")"
	expect "$1: create temporal entity" 201 "$status"
	db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = '$tenant'")"
	# History is partitioned on the write clock, so rows recorded sixty days ago
	# land in a chunk past both horizons; eight instances of the one stream make
	# it pay to convert rather than stay in the rowstore.
	sql "$db" "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
		INSERT INTO attribute_history SELECT
			now() - interval '60 days' + n * interval '1 minute', gen_random_uuid(), a.stream_key, a.write_ordinal, a.attribute_kind, a.value_kind,
			a.observed_at, a.created_at, a.modified_at, a.deleted_at, a.value_date_time, a.expires_at,
			CASE WHEN a.generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END,
			a.value_type_key, a.value_datatype_key, a.unit_code_id, a.value_boolean, a.verbatim_instance_id, a.value_number, a.value_string,
			a.value_language, a.value_geometry, a.value_geometry_valid, a.value_geometry_reach, a.value_geometry_members, a.value_object, a.value_uri,
			a.object_type_keys, a.proof_seals, a.sub_attributes
		FROM aged a CROSS JOIN generate_series(1, 8) n;" > /dev/null
	expect "$1: the aged chunk is in the rowstore" false "$(sql "$db" "$aged_chunks")"
	docker restart "$broker" > /dev/null
}

# usage: check_log LABEL
check_log() {
	local logs
	logs="$(docker logs "$broker" 2>&1)"
	if grep -qE 'columnstore-overdue|retention-overdue' <<< "$logs"; then
		fail "$1: the broker log reports an overdue chunk"
	else
		pass "$1: no overdue chunk reported"
	fi
	if grep -qE 'ERROR' <<< "$logs"; then
		fail "$1: broker log carries an error"
		grep -E 'ERROR' <<< "$logs" >&2
	else
		pass "$1: broker log carries no error"
	fi
}

run_case retention "$image" enabled
wait_for "retention: the wake loop loads the tenant and the aged chunk is dropped" none 300 "$db" "$aged_chunks"
wait_for "retention: the unloaded tenant's next wake is weeks ahead" weeks-ahead 300 andromeda "$wake_state"
check_log retention

run_case conversion "$image" disabled
wait_for "conversion: the wake loop loads the tenant and the aged chunk is converted" true 300 "$db" "$aged_chunks"
wait_for "conversion: the unloaded tenant's next wake is weeks ahead" weeks-ahead 300 andromeda "$wake_state"
check_log conversion

if [ -n "$apache_image" ]; then
	run_case apache "$apache_image" disabled
	wait_for "apache: the reconciliation probe records no wake" none 300 andromeda "$wake_state"
	sleep 60
	expect "apache: the aged chunk stays in the rowstore" false "$(sql "$db" "$aged_chunks")"
	expect "apache: still no wake" none "$(sql andromeda "$wake_state")"
	check_log apache
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
