#!/usr/bin/env bash
# usage: extension-update-check.sh IMAGE BROKER_IMAGE [PREVIOUS_IMAGE PREVIOUS_BROKER_IMAGE [SCALE_TENANTS]]
# Asserts that the broker updates each tenant database's TimescaleDB and
# PostGIS to the versions IMAGE ships, and installs its pg_repack at the
# version IMAGE ships, when it opens the tenant, and that the tenant then
# serves. The first case creates the databases with TimescaleDB at
# the previous minor release, which IMAGE still ships. The cases run when
# PREVIOUS_IMAGE and PREVIOUS_BROKER_IMAGE are given start from tenants the
# previous broker created and filled on the previous image. The first of them
# restarts the data directory on IMAGE under BROKER_IMAGE and sends no
# request: the wake pass alone must bring every tenant up to date, the one
# with the most committed transactions early, and record what each holds. It
# runs four times: with the broker reaching the server directly, with its
# requests served through a transaction-pooling PgBouncer, with the broker
# stopped and started again while it catches the tenants up, and with the
# previous broker serving half the tenants on IMAGE before BROKER_IMAGE
# replaces it. A second case replaces the previous image with IMAGE under a
# running BROKER_IMAGE, and a third starts upgraded data under the previous
# broker and on the previous image, which must refuse it and change nothing.
# With SCALE_TENANTS, a last case catches that many tenants up under the
# server's and the broker's default limits while a resident tenant serves.
set -Eeuo pipefail

image="$1"
broker_image="$2"
previous_image="${3:-}"
previous_broker_image="${4:-}"
scale_tenants="${5:-}"
pooler_image=pgbouncer/pgbouncer:1.25.2-20260911.206
prefix="extension-update-check-$$"
network="$prefix-net"
volume="$prefix-data"
database="$prefix-db"
broker="$prefix-broker"
pooler="$prefix-pooler"
mounts="$(mktemp -d)"
chmod 755 "$mounts"
# The environment every broker of the case running is started with.
broker_env=()
# The server settings every database of the case running is started with.
# Twenty tenants loaded on the previous broker's pools hold more connections
# than the default 200.
server_args=(-c max_connections=400)
# The log of a broker the running case stopped before its checks.
earlier_log="$mounts/earlier.log"
tenant=upd
failures=0
response="$(mktemp)"

cleanup() {
	docker rm -f -v "$broker" "$pooler" "$database" > /dev/null 2>&1 || :
	docker volume rm -f "$volume" > /dev/null 2>&1 || :
	docker network rm "$network" > /dev/null 2>&1 || :
	rm -f "$response"
	rm -rf "$mounts"
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
# Waits for the image's health check, which probes over TCP, which the
# socket-only server of first-start initialisation never opens. The server
# gets the 1GB of /dev/shm a deployment gives it: the cumulative statistics of
# hundreds of databases outgrow Docker's default 64MB.
start_database() {
	local image="$1"
	shift
	docker run -d --name "$database" --network "$network" --shm-size 1g -e POSTGRES_PASSWORD=check \
		-v "$volume:/var/lib/postgresql" "$@" "$image" postgres "${server_args[@]}" > /dev/null
	for _ in $(seq 1 600); do
		if [ "$(docker inspect -f '{{.State.Health.Status}}' "$database" 2> /dev/null)" = healthy ]; then
			return 0
		fi
		sleep 0.5
	done
	docker logs "$database" >&2
	printf 'FAIL %s not ready after 300s\n' "$database" >&2
	exit 1
}

# A server with hundreds of databases writes its shutdown checkpoint for longer
# than the ten seconds docker stop allows by default.
stop_database() {
	docker stop -t 300 "$database" > /dev/null
	docker rm -v "$database" > /dev/null
}

# usage: sql DATABASE QUERY
sql() {
	docker exec -u postgres "$database" psql -U andromeda -XAtq -v ON_ERROR_STOP=1 -d "$1" -c "$2"
}

# usage: sql_unloaded DATABASE QUERY
# As sql, in a session that never loads TimescaleDB, so it reads a database
# whose installed TimescaleDB library the server lacks.
sql_unloaded() {
	docker exec -u postgres -e PGOPTIONS='-c timescaledb.disable_load=on' "$database" \
		psql -U andromeda -XAtq -v ON_ERROR_STOP=1 -d "$1" -c "$2"
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
		"${broker_env[@]}" "$1" > /dev/null
	local port
	port="$(docker port "$broker" 1056/tcp)"
	base="http://${port}/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if docker exec "$broker" andromeda health > /dev/null 2>&1; then
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

# usage: start_refused IMAGE DESCRIPTION [LOG_TEXT]
# Starts a broker that must refuse the database it is given: it exits, or is
# still not serving after a minute, and its log names LOG_TEXT when given.
start_refused() {
	docker run -d --name "$broker" --network "$network" \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__CONNECTION=postgres://andromeda:check@$database:5432/andromeda" \
		"$1" > /dev/null
	local served=no
	for _ in $(seq 1 60); do
		if docker exec "$broker" andromeda health > /dev/null 2>&1; then
			served=yes
			break
		fi
		[ "$(docker inspect -f '{{.State.Running}}' "$broker")" = true ] || break
		sleep 1
	done
	expect "$2 does not serve" no "$served"
	if [ -n "${3:-}" ]; then
		expect "$2 names the refusal" yes "$(docker logs "$broker" 2>&1 | grep -qF -- "$3" && echo yes || echo no)"
	fi
	docker rm -f "$broker" > /dev/null
}

# A transaction-pooling PgBouncer in front of the server, with the server-side
# timings PgBouncer defaults to: a server session idle for less than ten
# minutes is kept and handed to the next client after its check query.
start_pooler() {
	mkdir -p "$mounts/pgbouncer"
	cat > "$mounts/pgbouncer/pgbouncer.ini" <<-INI
		[databases]
		* = host=$database port=5432
		[pgbouncer]
		listen_addr = 0.0.0.0
		listen_port = 6432
		unix_socket_dir = /tmp
		pool_mode = transaction
		auth_type = trust
		auth_file = /etc/pgbouncer/userlist.txt
		max_client_conn = 1000
		default_pool_size = 8
		max_db_connections = 40
		max_prepared_statements = 64
		ignore_startup_parameters = extra_float_digits,IntervalStyle,DateStyle,TimeZone,client_encoding
	INI
	printf '"andromeda" "check"\n' > "$mounts/pgbouncer/userlist.txt"
	chmod -R a+rX "$mounts/pgbouncer"
	docker run -d --name "$pooler" --network "$network" -v "$mounts/pgbouncer:/etc/pgbouncer:ro" \
		--entrypoint /opt/pgbouncer/pgbouncer "$pooler_image" /etc/pgbouncer/pgbouncer.ini > /dev/null
	for _ in $(seq 1 60); do
		if docker exec "$pooler" nc -z 127.0.0.1 6432 > /dev/null 2>&1; then
			return 0
		fi
		sleep 0.5
	done
	docker logs "$pooler" >&2
	printf 'FAIL %s not listening after 30s\n' "$pooler" >&2
	exit 1
}

stop_pooler() {
	docker rm -f "$pooler" > /dev/null
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

# usage: serving_again ENTITY_SUFFIX
# Waits up to a minute for the tenant to serve its entity again, as the
# broker's pools reconnect to a server that was replaced, then reads back what
# write_tenant wrote.
serving_again() {
	for _ in $(seq 1 60); do
		[ "$(status GET "/entities/urn:ngsi-ld:Probe:$1")" = 200 ] && break
		sleep 1
	done
	read_tenant "$1"
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
	log="$(cat "$earlier_log" 2> /dev/null; docker logs "$broker" 2>&1)"
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

# usage: current_tenants; prints how many catch-up tenants hold the shipped
# TimescaleDB and PostGIS.
current_tenants() {
	local name db count=0
	for name in "${catch_up_tenants[@]}"; do
		db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = '$name'")"
		if [ "$(extversion "$db" timescaledb)" = "$(shipped timescaledb)" ] && [ "$(extversion "$db" postgis)" = "$(shipped postgis)" ]; then
			count=$((count + 1))
		fi
	done
	printf '%s' "$count"
}

# usage: tenant_db NAME
tenant_db() {
	sql andromeda "SELECT database_name FROM tenants WHERE name = '$1'"
}

# usage: lagging_tenants; prints, comma-separated and quoted for SQL, the
# catch-up tenants whose TimescaleDB or PostGIS stands below the version the
# server ships.
lagging_tenants() {
	local name db listed=''
	for name in "${catch_up_tenants[@]}"; do
		db="$(tenant_db "$name")"
		if [ "$(extversion "$db" timescaledb)" != "$(shipped timescaledb)" ] || [ "$(extversion "$db" postgis)" != "$(shipped postgis)" ]; then
			listed="$listed${listed:+,}'$name'"
		fi
	done
	printf '%s' "$listed"
}

# usage: lagging_pairs; prints how many TimescaleDB and PostGIS installations
# in the platform database and the catch-up tenants' databases stand below the
# versions the server ships, which is how many updates bring them up to date.
lagging_pairs() {
	local db extension count=0
	for db in andromeda $(for name in "${catch_up_tenants[@]}"; do tenant_db "$name"; done); do
		for extension in timescaledb postgis; do
			[ "$(extversion "$db" "$extension")" = "$(shipped "$extension")" ] || count=$((count + 1))
		done
	done
	printf '%s' "$count"
}

# usage: fingerprint; prints the server's databases with their identity, and
# for each database the broker uses its extension versions, migration ledger
# and schema stamp, read without loading TimescaleDB.
fingerprint() {
	local db
	sql_unloaded andromeda "SELECT string_agg(datname || ':' || oid, ',' ORDER BY datname) FROM pg_database"
	for db in $(sql_unloaded andromeda "SELECT datname FROM pg_database WHERE datallowconn AND datname LIKE 'andromeda%' ORDER BY datname"); do
		printf '%s ' "$db"
		sql_unloaded "$db" "SELECT (SELECT string_agg(extname || '=' || extversion, ',' ORDER BY extname) FROM pg_extension) || ' ' || (SELECT max(version) || ':' || md5(string_agg(version || encode(checksum, 'hex') || success, ',' ORDER BY version)) FROM _andromeda_tenant_migrations) || ' ' || (SELECT md5(string_agg(m::text, ',')) FROM schema_metadata m)"
	done
	sql_unloaded andromeda "SELECT max(version) || ':' || md5(string_agg(version || encode(checksum, 'hex') || success, ',' ORDER BY version)) FROM _andromeda_platform_migrations"
}

# usage: record_mismatches; prints the catch-up tenants and extensions whose
# recorded installed version differs from the tenant database's catalogue.
record_mismatches() {
	local name db extension recorded mismatched=''
	for name in "${catch_up_tenants[@]}"; do
		db="$(tenant_db "$name")"
		for extension in timescaledb postgis pg_repack; do
			recorded="$(sql andromeda "SELECT v.installed_version FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name = '$name' AND v.extension = '$extension'")"
			[ "$recorded" = "$(extversion "$db" "$extension")" ] || mismatched="$mismatched $name/$extension"
		done
	done
	printf '%s' "$mismatched"
}

# usage: previous_release_case direct|pooled|interrupted|image-first|workers-on
previous_release_case() {
	printf -- '--- tenants created on the previous image, brought up to date on this one with no request, %s\n' "$1"
	broker_env=()
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
	stop_database
	case "$1" in
		workers-on)
			# A deployment that asked the previous release for its 64 workers.
			start_database "$image" -e TS_TUNE_MAX_BG_WORKERS=64
			expect "the previous release's server keeps the workers it asks for" 64 "$(sql andromeda 'SHOW timescaledb.max_background_workers')"
			;;
		*)
			start_database "$image"
			# The image tunes the data directory the previous release left on
			# every start, replacing the block of overrides that release appended.
			expect "the previous release's server runs no TimescaleDB background worker" 0 "$(sql andromeda 'SHOW timescaledb.max_background_workers')"
			;;
	esac
	expect "$first_db timescaledb on the new image before the broker" "$old_timescaledb" "$(extversion "$first_db" timescaledb)"
	# template0 records no collation version, and the server warns only about a
	# recorded one that differs from what its C library reports.
	expect "databases whose recorded collation version differs from the server's" 0 \
		"$(sql andromeda 'SELECT count(*) FROM pg_database WHERE datcollversion <> pg_database_collation_actual_version(oid)')"
	expect "the previous release's postgresql.conf holds one block of overrides" 1 \
		"$(docker exec "$database" sh -c 'grep -c "^# --- andromeda overrides BEGIN ---\$" "$PGDATA/postgresql.conf"')"
	if [ "$1" = pooled ]; then
		start_pooler
		broker_env=(
			-e "ANDROMEDA__DATABASE__POOLER__CONNECTION=postgres://andromeda:check@$pooler:6432/andromeda"
			-e ANDROMEDA__DATABASE__POOLER__MAX_CONNECTIONS_PER_TENANT=2
		)
	fi
	if [ "$1" = image-first ]; then
		# The image is upgraded under the previous broker, which is sent a
		# request for each of the first half of the tenants and left to its
		# own wake pass for a while before this broker replaces it. What the
		# previous release answers on a newer image is its own; this broker
		# must bring whatever it left up to date and record it.
		start_broker "$previous_broker_image"
		for name in "${catch_up_tenants[@]:0:10}"; do
			tenant="$name"
			status GET /entities/urn:ngsi-ld:Probe:b > /dev/null
		done
		sleep 10
		stop_broker
	fi
	# Each lagging extension is updated once in its database.
	updates="$(lagging_pairs)"
	behind_at_start="$(lagging_tenants)"
	# A resident tenant's background work commits far more than a client's
	# writes, so the busiest is read from the counter the catch-up ranks by.
	busiest=''
	if [ -n "$behind_at_start" ]; then
		busiest="$(sql andromeda "SELECT t.name FROM tenants t JOIN pg_stat_database d ON d.datname = t.database_name WHERE t.name IN ($behind_at_start) ORDER BY d.xact_commit DESC LIMIT 1")"
	fi
	catch_up_start="$(sql andromeda 'SELECT now()')"
	start_broker "$broker_image"
	assert_current andromeda
	if [ "$1" = interrupted ]; then
		# Stopped once a quarter of the tenants are current, possibly within an
		# update, the broker leaves the rest to its next start.
		for _ in $(seq 1 90); do
			[ "$(current_tenants)" -ge 5 ] && break
			sleep 0.2
		done
		docker logs "$broker" > "$earlier_log" 2>&1
		stop_broker
		start_broker "$broker_image"
	fi
	# Five wake passes of four tenants, plus building each, with no request.
	# A tenant is behind while its catalogue, or the record of a tenant this
	# broker caught up, holds an older version than the server ships.
	lagging=''
	for _ in $(seq 1 90); do
		lagging=''
		for name in "${catch_up_tenants[@]}"; do
			db="$(tenant_db "$name")"
			for extension in timescaledb postgis; do
				recorded="$(sql andromeda "SELECT v.installed_version FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name = '$name' AND v.extension = '$extension'")"
				if [ "$(extversion "$db" "$extension")" != "$(shipped "$extension")" ] || { [ "$1" != image-first ] && [ "$recorded" != "$(shipped "$extension")" ]; }; then
					lagging="$lagging $name/$extension"
				fi
			done
		done
		[ -z "$lagging" ] && break
		sleep 1
	done
	expect "tenants still behind the image after 90s with no request" '' "$lagging"
	expect "sessions on the tenant template" 0 \
		"$(sql andromeda "SELECT count(*) FROM pg_stat_activity a JOIN pg_database d ON d.datname = a.datname WHERE d.datistemplate")"
	# The record matches every tenant database's own catalogue. A record the
	# previous broker wrote on this image is corrected when the tenant next
	# opens, which the reads below do.
	if [ "$1" != image-first ]; then
		expect "tenants whose recorded versions differ from pg_extension" '' "$(record_mismatches)"
	fi
	# Scheduled wakes take a pass's four loads first, so the busiest tenant is
	# among the tenants the first two passes open.
	if [ -n "$busiest" ]; then
		busiest_rank="$(sql andromeda "SELECT count(*) FROM (SELECT min(v.reconciled_at) AS opened FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name IN ($behind_at_start) AND v.reconciled_at >= '$catch_up_start' GROUP BY t.name) o WHERE o.opened <= (SELECT min(v.reconciled_at) FROM tenant_extension_versions v JOIN tenants t USING (tenant_key) WHERE t.name = '$busiest' AND v.reconciled_at >= '$catch_up_start')")"
		expect "$busiest opened within the first eight tenants" t "$([ "$busiest_rank" -le 8 ] && echo t || echo "f, opened as number $busiest_rank")"
	fi
	assert_broker_log_clean "$updates"
	# Every tenant serves after the catch-up, on sessions its pool opened after
	# the update, and no request runs a further update.
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		read_tenant b
	done
	expect "tenants whose recorded versions differ from pg_extension after a request" '' "$(record_mismatches)"
	tenant=cu01
	write_tenant c
	assert_broker_log_clean "$updates"
	# A broker started again on the upgraded server updates nothing and loads no
	# tenant for its extensions.
	rm -f "$earlier_log"
	stop_broker
	start_broker "$broker_image"
	tenant=cu02
	read_tenant b
	assert_broker_log_clean 0
	expect "tenants listed behind the image after the restart" 0 \
		"$(sql andromeda "SELECT count(*) FROM tenant_extension_versions v JOIN pg_available_extensions a ON a.name = v.extension WHERE v.installed_version IS DISTINCT FROM a.default_version")"
	stop_broker
	if [ "$1" = pooled ]; then
		stop_pooler
	fi
	stop_database
	docker volume rm "$volume" > /dev/null
}

# usage: outside_window FROM UNTIL < LOG
# Prints the log lines stamped before FROM or after UNTIL, both UTC instants
# to the second, and every update the broker logged in between.
outside_window() {
	awk -v from="$1" -v until="$2" '{
		line = $0
		gsub(/\033\[[0-9;]*m/, "", line)
		split(line, field, " ")
		if (field[1] < from || field[1] > until || line ~ /extension_updated/) print line
	}'
}

# usage: custom_role_case
# A data directory the previous image initialised under its own superuser and
# database names, started on this image with the same environment: the health
# check connects as that role to that database.
custom_role_case() {
	printf -- '--- a data directory with a custom superuser and database, on this image\n'
	start_database "$previous_image" -e POSTGRES_USER=custom -e POSTGRES_DB=customdb
	stop_database
	start_database "$image" -e POSTGRES_USER=custom -e POSTGRES_DB=customdb
	expect "the health check connects as POSTGRES_USER to POSTGRES_DB" no \
		"$(docker logs "$database" 2>&1 | grep -q 'does not exist' && echo yes || echo no)"
	stop_database
	docker volume rm "$volume" > /dev/null
}

# usage: live_swap_case
# The broker is upgraded first, on the previous image, and the image is then
# replaced under it while it runs.
live_swap_case() {
	printf -- '--- the previous image replaced by this one under a running broker\n'
	mapfile -t catch_up_tenants < <(seq -f 'lv%02g' 1 12)
	start_database "$previous_image" -e POSTGRES_USER=andromeda -e POSTGRES_DB=andromeda
	# Four resident tenants, so most tenants are opened again after the swap.
	broker_env=(-e ANDROMEDA__TENANCY__RESIDENCY__MAX_RESIDENT=4)
	start_broker "$broker_image"
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		write_tenant b
	done
	template="$(sql andromeda "SELECT datname FROM pg_database WHERE datistemplate AND datname NOT IN ('template0', 'template1')")"
	template_oid="$(sql andromeda "SELECT oid FROM pg_database WHERE datname = '$template'")"
	old_timescaledb="$(shipped timescaledb)"
	old_postgis="$(shipped postgis)"
	swapped_at="$(date -u +%Y-%m-%dT%H:%M:%S)"
	stop_database
	start_database "$image"
	changed=0
	[ "$old_timescaledb" = "$(shipped timescaledb)" ] || changed=$((changed + 1))
	[ "$old_postgis" = "$(shipped postgis)" ] || changed=$((changed + 1))
	expect "the broker kept running while the server was replaced" true "$(docker inspect -f '{{.State.Running}}' "$broker")"
	# Every tenant serves again once the pools reconnect, and a tenant opened
	# again on the new server is brought up to date when it opens.
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		serving_again b
	done
	recovered_at="$(date -u +%Y-%m-%dT%H:%M:%S)"
	for name in "${catch_up_tenants[@]}"; do
		assert_current "$(tenant_db "$name")"
	done
	# A tenant created now is copied from the template the previous image's
	# versions are installed in, and brought up to date when it opens.
	tenant=lvmid
	write_tenant m
	read_tenant m
	assert_current "$(tenant_db lvmid)"
	# The platform database, the default tenant's, keeps its versions until
	# the broker starts again.
	expect "the platform database's timescaledb while the broker runs" "$old_timescaledb" "$(extversion andromeda timescaledb)"
	# Until the pools reconnect, the broker's readiness and the requests it
	# was serving fail. No session may meet a stale library at any time.
	expect "sessions closed for a stale library across the replacement" 0 \
		"$(docker logs "$broker" 2>&1 | grep -c 'already loaded with a different version' || :)"
	docker logs "$broker" 2>&1 | outside_window "$swapped_at" "$recovered_at" > "$earlier_log"
	stop_broker
	broker_env=()
	start_broker "$broker_image"
	assert_current andromeda
	expect "the template kept its identity" "$template_oid" "$(sql andromeda "SELECT oid FROM pg_database WHERE datname = '$template'")"
	expect "the template stays locked" "t f" "$(sql andromeda "SELECT datistemplate || ' ' || datallowconn FROM pg_database WHERE datname = '$template'" | sed 's/true/t/; s/false/f/')"
	tenant=lvnew
	write_tenant c
	assert_current "$(tenant_db lvnew)"
	# The platform database, twelve tenants, the one created after the swap
	# and the template, each updated once.
	assert_broker_log_clean $((changed * (1 + ${#catch_up_tenants[@]} + 1 + 1)))
	rm -f "$earlier_log"
	stop_broker
	stop_database
	docker volume rm "$volume" > /dev/null
}

# usage: assert_unchanged DESCRIPTION
# Compares the server's fingerprint with the one taken of the upgraded data.
assert_unchanged() {
	fingerprint > "$mounts/now"
	if cmp -s "$mounts/upgraded" "$mounts/now"; then
		pass "$1"
	else
		fail "$1"
		diff "$mounts/upgraded" "$mounts/now" >&2 || :
	fi
}

# usage: downgrade_case
# Data brought up to date on this image under this broker, then started under
# the previous broker, on the previous image under either broker, and on this
# image again.
downgrade_case() {
	printf -- '--- upgraded data given to the previous broker and the previous image\n'
	broker_env=()
	mapfile -t catch_up_tenants < <(seq -f 'dg%02g' 1 4)
	start_database "$previous_image" -e POSTGRES_USER=andromeda -e POSTGRES_DB=andromeda
	start_broker "$previous_broker_image"
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		write_tenant b
	done
	stop_broker
	stop_database
	start_database "$image"
	start_broker "$broker_image"
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		read_tenant b
		assert_current "$(tenant_db "$name")"
	done
	stop_broker
	fingerprint > "$mounts/upgraded"
	start_refused "$previous_broker_image" 'the previous broker on upgraded data'
	assert_unchanged "the previous broker leaves upgraded data as it was"
	stop_database
	start_database "$previous_image"
	start_refused "$broker_image" 'this broker on the previous image' 'is installed, but the server ships'
	assert_unchanged "this broker on the previous image leaves the data as it was"
	start_refused "$previous_broker_image" 'the previous broker on the previous image'
	assert_unchanged "the previous broker on the previous image leaves the data as it was"
	stop_database
	start_database "$image"
	assert_unchanged "the data back on this image is as this broker left it"
	start_broker "$broker_image"
	for name in "${catch_up_tenants[@]}"; do
		tenant="$name"
		read_tenant b
	done
	assert_broker_log_clean 0
	stop_broker
	stop_database
	docker volume rm "$volume" > /dev/null
}

# usage: behind_catalogue; prints how many TimescaleDB and PostGIS
# installations in the scale tenants' databases stand below the versions the
# server ships, read from each database's own catalogue.
behind_catalogue() {
	docker exec -i -u postgres "$database" sh -s <<-'SH'
		total=0
		for db in $(psql -U andromeda -XAt -d andromeda -c "SELECT database_name FROM tenants WHERE name LIKE 'sc%'"); do
			behind=$(psql -U andromeda -XAt -d "$db" -c "SELECT count(*) FROM pg_extension e JOIN pg_available_extensions a ON a.name = e.extname WHERE e.extname IN ('timescaledb', 'postgis') AND e.extversion <> a.default_version")
			total=$((total + behind))
		done
		printf '%s' "$total"
	SH
}

# usage: scale_case COUNT
# COUNT tenants the previous broker created on the previous image, caught up
# with no request on this image under the server's default connection limit
# and the broker's default residency and wake limits, while a resident tenant
# serves a request every second.
scale_case() {
	printf -- '--- %s tenants created on the previous image, brought up to date under the default limits\n' "$1"
	broker_env=()
	mapfile -t catch_up_tenants < <(seq -f 'sc%04g' 1 "$1")
	start_database "$previous_image" -e POSTGRES_USER=andromeda -e POSTGRES_DB=andromeda
	start_broker "$previous_broker_image"
	printf '%s\n' "${catch_up_tenants[@]}" | xargs -P 8 -I '{}' curl -sS -o /dev/null -w '%{http_code}\n' --retry 30 --retry-delay 1 \
		-X POST -H 'NGSILD-Tenant: {}' -H 'Content-Type: application/json' \
		--data '{"id":"urn:ngsi-ld:Probe:b","type":"Probe","temperature":{"type":"Property","value":21}}' \
		"$base/entities" > "$mounts/created"
	expect "tenants the previous broker created" "$1" "$(grep -cE '^(201|409)$' "$mounts/created" || :)"
	stop_broker
	old_timescaledb="$(shipped timescaledb)"
	old_postgis="$(shipped postgis)"
	stop_database
	server_args=()
	start_database "$image"
	changed=0
	[ "$old_timescaledb" = "$(shipped timescaledb)" ] || changed=$((changed + 1))
	[ "$old_postgis" = "$(shipped postgis)" ] || changed=$((changed + 1))
	printf 'server max_connections %s\n' "$(sql andromeda 'SHOW max_connections')"
	start_broker "$broker_image"
	tenant=hot
	write_tenant h
	: > "$mounts/hot"
	local started elapsed remaining
	started="$(date +%s)"
	while :; do
		status GET /entities/urn:ngsi-ld:Probe:h >> "$mounts/hot"
		printf '\n' >> "$mounts/hot"
		elapsed=$(($(date +%s) - started))
		remaining="$(sql andromeda "SELECT count(*) FROM tenants t WHERE t.name LIKE 'sc%' AND (SELECT count(*) FROM tenant_extension_versions v JOIN pg_available_extensions a ON a.name = v.extension WHERE v.tenant_key = t.tenant_key AND v.extension IN ('timescaledb', 'postgis') AND v.installed_version = a.default_version) < 2")"
		[ "$remaining" -eq 0 ] && break
		[ "$elapsed" -ge 3600 ] && break
		sleep 1
	done
	printf 'caught up %s of %s tenants in %ss\n' "$(($1 - remaining))" "$1" "$elapsed"
	expect "tenants the record lists behind the image after the catch-up" 0 "$remaining"
	expect "installations behind the image in the tenants' catalogues" 0 "$(behind_catalogue)"
	expect "requests to the resident tenant answered other than 200 during the catch-up" 0 "$(grep -cv '^200$' "$mounts/hot" || :)"
	expect "connections the server refused for its limit" 0 "$(docker logs "$database" 2>&1 | grep -cE 'too many clients|remaining connection slots' || :)"
	assert_broker_log_clean $((changed * (1 + $1)))
	stop_broker
	stop_database
	docker volume rm "$volume" > /dev/null
	server_args=(-c max_connections=400)
}

if [ -n "$previous_image" ] && [ -n "$previous_broker_image" ]; then
	previous_release_case direct
	previous_release_case pooled
	previous_release_case interrupted
	previous_release_case image-first
	previous_release_case workers-on
	custom_role_case
	live_swap_case
	downgrade_case
	if [ -n "$scale_tenants" ]; then
		scale_case "$scale_tenants"
	fi
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
