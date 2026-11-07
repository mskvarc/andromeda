#!/usr/bin/env bash
# usage: extension-update-check.sh IMAGE BROKER_IMAGE [PREVIOUS_IMAGE PREVIOUS_BROKER_IMAGE]
# Asserts that the broker updates each tenant database's TimescaleDB and
# PostGIS to the versions IMAGE ships, and installs its pg_repack at the
# version IMAGE ships, when it opens the tenant, and that the tenant then
# serves. The first case creates the databases with TimescaleDB at
# the previous minor release, which IMAGE still ships. The second, run when
# PREVIOUS_IMAGE and PREVIOUS_BROKER_IMAGE are given, creates and fills twenty
# tenants with the previous broker on the previous image, then restarts the
# data directory on IMAGE under BROKER_IMAGE and sends no request: the wake
# pass alone must bring every tenant up to date, the one with the most
# committed transactions early, and record what each holds.
set -Eeuo pipefail

image="$1"
broker_image="$2"
previous_image="${3:-}"
previous_broker_image="${4:-}"
prefix="extension-update-check-$$"
network="$prefix-net"
volume="$prefix-data"
database="$prefix-db"
broker="$prefix-broker"
tenant=upd
failures=0
response="$(mktemp)"

cleanup() {
	docker rm -f "$broker" "$database" > /dev/null 2>&1 || :
	docker volume rm -f "$volume" > /dev/null 2>&1 || :
	docker network rm "$network" > /dev/null 2>&1 || :
	rm -f "$response"
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

# usage: start_database IMAGE [docker-run-args...]
# Waits for TCP, which the socket-only server of first-start initialisation never opens.
# Twenty tenants loaded on the previous broker's pools hold more connections than the default 200.
start_database() {
	local image="$1"
	shift
	docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check \
		-v "$volume:/var/lib/postgresql" "$@" "$image" postgres -c max_connections=400 > /dev/null
	for _ in $(seq 1 120); do
		if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
			return 0
		fi
		sleep 0.5
	done
	docker logs "$database" >&2
	printf 'FAIL %s not ready after 60s\n' "$database" >&2
	exit 1
}

stop_database() {
	docker stop "$database" > /dev/null
	docker rm "$database" > /dev/null
}

# usage: sql DATABASE QUERY
sql() {
	docker exec -u postgres "$database" psql -U andromeda -XAtq -v ON_ERROR_STOP=1 -d "$1" -c "$2"
}

# usage: extversion DATABASE EXTENSION
extversion() {
	sql "$1" "SELECT extversion FROM pg_extension WHERE extname = '$2'"
}

# usage: shipped EXTENSION
shipped() {
	sql andromeda "SELECT default_version FROM pg_available_extensions WHERE name = '$1'"
}

tenant_database() {
	sql andromeda "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'"
}

# usage: start_broker IMAGE
start_broker() {
	docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__CONNECTION=postgres://andromeda:check@$database:5432/andromeda" \
		"$1" > /dev/null
	local port
	port="$(docker port "$broker" 1056/tcp)"
	base="http://${port}/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
			return 0
		fi
		if [ "$(docker inspect -f '{{.State.Running}}' "$broker")" != true ]; then
			docker logs "$broker" >&2
			printf 'FAIL %s exited before serving\n' "$broker" >&2
			exit 1
		fi
		sleep 0.5
	done
	docker logs "$broker" >&2
	printf 'FAIL %s not healthy after 60s\n' "$broker" >&2
	exit 1
}

stop_broker() {
	docker stop "$broker" > /dev/null
	docker rm "$broker" > /dev/null
}

# usage: status METHOD PATH [BODY]
# Prints the HTTP status of one request to the tenant; the body lands in $response.
status() {
	local args=( -sS -o "$response" -w '%{http_code}' -X "$1" -H "NGSILD-Tenant: $tenant" )
	if [ -n "${3:-}" ]; then
		args+=( -H 'Content-Type: application/json' --data "$3" )
	fi
	curl "${args[@]}" "$base$2"
}

# usage: write_tenant ENTITY_SUFFIX
# Creates an entity and a temporal entity with two instances in the tenant.
write_tenant() {
	expect "create entity Probe:$1" 201 "$(status POST /entities "{\"id\":\"urn:ngsi-ld:Probe:$1\",\"type\":\"Probe\",\"temperature\":{\"type\":\"Property\",\"value\":21}}")"
	expect "create temporal entity Series:$1" 201 "$(status POST /temporal/entities "{\"id\":\"urn:ngsi-ld:Series:$1\",\"type\":\"Series\",\"temperature\":[{\"type\":\"Property\",\"value\":1,\"observedAt\":\"2026-10-17T10:00:00Z\"},{\"type\":\"Property\",\"value\":2,\"observedAt\":\"2026-10-17T11:00:00Z\"}]}")"
}

# usage: read_tenant ENTITY_SUFFIX
# Reads back what write_tenant wrote.
read_tenant() {
	expect "retrieve entity Probe:$1" 200 "$(status GET "/entities/urn:ngsi-ld:Probe:$1")"
	expect "retrieve temporal entity Series:$1" 200 "$(status GET "/temporal/entities/urn:ngsi-ld:Series:$1")"
	local instances
	instances="$(grep -o '"observedAt"' "$response" | wc -l | tr -d ' ')"
	expect "temporal instances of Series:$1" 2 "$instances"
}

# usage: assert_current DATABASE
assert_current() {
	expect "$1 timescaledb" "$(shipped timescaledb)" "$(extversion "$1" timescaledb)"
	expect "$1 postgis" "$(shipped postgis)" "$(extversion "$1" postgis)"
	expect "$1 pg_repack" "$(shipped pg_repack)" "$(extversion "$1" pg_repack)"
}

# An ERROR level, a session TimescaleDB closed for a stale library, or an update
# the broker's role was refused.
log_errors='ERROR|already loaded with a different version|extension_update_refused'

assert_broker_log_clean() {
	local log
	log="$(docker logs "$broker" 2>&1)"
	if grep -qE "$log_errors" <<< "$log"; then
		fail "broker log carries an error"
		grep -E "$log_errors" <<< "$log" >&2
	else
		pass "broker log carries no error"
	fi
	expect "extension_updated events" "$1" "$(grep -c 'extension_updated' <<< "$log" || :)"
}

docker network create "$network" > /dev/null

printf -- '--- previous minor created in the platform database and template1\n'
start_database "$image"
current="$(shipped timescaledb)"
previous="$(sql andromeda "SELECT version FROM pg_available_extension_versions WHERE name = 'timescaledb' AND version LIKE '${current%.*.*}.$(( $(cut -d. -f2 <<< "$current") - 1 )).%'" | sort -V | tail -n 1)"
if [ -z "$previous" ]; then
	printf 'FAIL %s ships no release of the minor before %s\n' "$image" "$current" >&2
	exit 1
fi
for name in andromeda template1; do
	sql "$name" "CREATE EXTENSION timescaledb VERSION '$previous'"
	sql "$name" "CREATE EXTENSION postgis"
	expect "$name timescaledb before the broker" "$previous" "$(extversion "$name" timescaledb)"
done
start_broker "$broker_image"
assert_current andromeda
write_tenant a
read_tenant a
assert_current "$(tenant_database)"
assert_broker_log_clean 2
stop_broker
stop_database
docker volume rm "$volume" > /dev/null

if [ -n "$previous_image" ] && [ -n "$previous_broker_image" ]; then
	printf -- '--- tenants created on the previous image, brought up to date on this one with no request\n'
	mapfile -t catch_up_tenants < <(seq -f 'cu%02g' 1 20)
	# An image older than the andromeda default role is told to create it.
	start_database "$previous_image" -e POSTGRES_USER=andromeda -e POSTGRES_DB=andromeda
	start_broker "$previous_broker_image"
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		write_tenant b
	done
	stop_broker
	first_db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = 'cu01'")"
	old_timescaledb="$(extversion "$first_db" timescaledb)"
	old_postgis="$(extversion "$first_db" postgis)"
	stop_database
	start_database "$image"
	expect "$first_db timescaledb on the new image before the broker" "$old_timescaledb" "$(extversion "$first_db" timescaledb)"
	# A resident tenant's background work commits far more than a client's
	# writes, so the busiest is read from the counter the catch-up ranks by.
	busiest="$(sql andromeda "SELECT t.name FROM tenants t JOIN pg_stat_database d ON d.datname = t.database_name WHERE t.name LIKE 'cu%' ORDER BY d.xact_commit DESC LIMIT 1")"
	start_broker "$broker_image"
	assert_current andromeda
	# Five wake passes of four tenants, plus building each, with no request.
	lagging=''
	for _ in $(seq 1 90); do
		lagging=''
		for name in "${catch_up_tenants[@]}"; do
			db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = '$name'")"
			if [ "$(extversion "$db" timescaledb)" != "$(shipped timescaledb)" ] || [ "$(extversion "$db" postgis)" != "$(shipped postgis)" ]; then
				lagging="$lagging $name"
			fi
		done
		[ -z "$lagging" ] && break
		sleep 1
	done
	expect "tenants still behind the image after 90s with no request" '' "$lagging"
	# The record matches every tenant database's own catalogue.
	mismatched=''
	for name in "${catch_up_tenants[@]}"; do
		db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = '$name'")"
		for extension in timescaledb postgis pg_repack; do
			recorded="$(sql andromeda "SELECT v.installed_version FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name = '$name' AND v.extension = '$extension'")"
			[ "$recorded" = "$(extversion "$db" "$extension")" ] || mismatched="$mismatched $name/$extension"
		done
	done
	expect "tenants whose recorded versions differ from pg_extension" '' "$mismatched"
	# Scheduled wakes take a pass's four loads first, so the busiest tenant is
	# among the tenants the first two passes open.
	busiest_rank="$(sql andromeda "SELECT count(*) FROM (SELECT min(v.reconciled_at) AS opened FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name LIKE 'cu%' GROUP BY t.name) o WHERE o.opened <= (SELECT min(v.reconciled_at) FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name = '$busiest')")"
	expect "$busiest opened within the first eight tenants" t "$([ "$busiest_rank" -le 8 ] && echo t || echo "f, opened as number $busiest_rank")"
	# Each changed extension is updated once in the platform database and once
	# in every tenant's.
	updates=0
	[ "$old_timescaledb" != "$(shipped timescaledb)" ] && updates=$((updates + 1 + ${#catch_up_tenants[@]}))
	[ "$old_postgis" != "$(shipped postgis)" ] && updates=$((updates + 1 + ${#catch_up_tenants[@]}))
	assert_broker_log_clean "$updates"
	# A request after the catch-up runs no further update.
	tenant=cu01
	read_tenant b
	write_tenant c
	assert_broker_log_clean "$updates"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
