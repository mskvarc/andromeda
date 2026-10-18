#!/usr/bin/env bash
# usage: settings-check.sh IMAGE [docker-run-args...]
# Starts IMAGE under container limits and asserts the settings the entrypoint
# and timescaledb-tune produce. Extra arguments go to every `docker run`.
set -Eeuo pipefail

image="$1"
shift
run_args=( "$@" )
prefix="settings-check-$$"
volume="$prefix-data"
containers=()
failures=0

cleanup() {
	if [ "${#containers[@]}" -gt 0 ]; then
		docker rm -f "${containers[@]}" > /dev/null 2>&1 || :
	fi
	docker volume rm -f "$volume" > /dev/null 2>&1 || :
}
trap cleanup EXIT

# usage: wait_ready CONTAINER
# Waits for TCP, which the socket-only server of first-start initialisation never opens.
wait_ready() {
	for _ in $(seq 1 120); do
		if docker exec "$1" pg_isready -q -h 127.0.0.1 2> /dev/null; then
			return 0
		fi
		if [ "$(docker inspect -f '{{.State.Running}}' "$1")" != true ]; then
			docker logs "$1" >&2
			printf 'FAIL %s exited before accepting connections\n' "$1" >&2
			exit 1
		fi
		sleep 0.5
	done
	docker logs "$1" >&2
	printf 'FAIL %s not ready after 60s\n' "$1" >&2
	exit 1
}

# usage: start NAME [docker-run-args...] -- [server-args...]
start() {
	local name="$prefix-$1"
	shift
	local docker_args=()
	while [ "$1" != -- ]; do
		docker_args+=( "$1" )
		shift
	done
	shift
	containers+=( "$name" )
	docker run -d --name "$name" -e POSTGRES_PASSWORD=check \
		"${run_args[@]}" "${docker_args[@]}" "$image" postgres "$@" > /dev/null
	wait_ready "$name"
}

# usage: sql NAME QUERY
sql() {
	docker exec -u postgres "$prefix-$1" psql -XAtq -c "$2"
}

# usage: setting_mb NAME SETTING
setting_mb() {
	sql "$1" "SELECT pg_size_bytes(current_setting('$2')) / 1048576"
}

# usage: sourcefile NAME SETTING
sourcefile() {
	sql "$1" "SELECT coalesce(sourcefile, source) FROM pg_settings WHERE name = '$2'"
}

# usage: expect LABEL ACTUAL EXPECTED
expect() {
	if [ "$2" = "$3" ]; then
		printf 'ok   %s\n' "$1"
	else
		printf 'FAIL %s: got %s, want %s\n' "$1" "$2" "$3"
		failures=$(( failures + 1 ))
	fi
}

# usage: expect_true LABEL VALUE; VALUE is 1 when the condition holds.
expect_true() {
	if [ "$2" = 1 ]; then
		printf 'ok   %s\n' "$1"
	else
		printf 'FAIL %s\n' "$1"
		failures=$(( failures + 1 ))
	fi
}

# usage: logs_mention NAME TEXT; prints yes or no.
logs_mention() {
	local log
	log="$(docker logs "$prefix-$1" 2>&1)"
	if [[ "$log" == *"$2"* ]]; then
		printf 'yes'
	else
		printf 'no'
	fi
}

budget_warning='warning: memory ceilings'

start limits-2g --memory 2g --cpus 2 --
# timescaledb-tune: shared_buffers 25% and effective_cache_size 75% of memory.
expect '2g shared_buffers' "$(setting_mb limits-2g shared_buffers)" 512
expect '2g effective_cache_size' "$(setting_mb limits-2g effective_cache_size)" 1536
expect '2g max_parallel_workers' "$(sql limits-2g 'SHOW max_parallel_workers')" 2
expect '2g io_workers' "$(sql limits-2g 'SHOW io_workers')" 3
# 5% of 2048MB over the tuner's 10 workers is 10MB, below the 32MB floor.
expect '2g autovacuum_work_mem' "$(setting_mb limits-2g autovacuum_work_mem)" 32
# A 32nd of 512MB shared_buffers is 16MB, raised to the 32MB floor.
expect '2g wal_buffers' "$(setting_mb limits-2g wal_buffers)" 32
expect '2g timescaledb.max_background_workers default' \
	"$(sql limits-2g 'SHOW timescaledb.max_background_workers')" 64
# The tuner sizes work_mem without hash_mem_multiplier or autovacuum:
# 512MB + 25 x 32MB x 2 + 10 x 32MB + 256MB.
expect '2g logs the tuner'"'"'s own ceilings' \
	"$(logs_mention limits-2g "$budget_warning of 2688MB exceed the 2048MB available")" yes

# TimescaleDB registers its settings only in a database holding the extension.
sql limits-2g 'CREATE EXTENSION IF NOT EXISTS timescaledb' > /dev/null
if docker exec "$prefix-limits-2g" sh -c 'ls "$(pg_config --pkglibdir)" | grep -q "^timescaledb-tsl-"'; then
	license=timescale
else
	license=apache
fi
expect "timescaledb.license matches the installed edition" "$(sql limits-2g 'SHOW timescaledb.license')" "$license"
policy_keys="$(docker exec "$prefix-limits-2g" sed -n 's/^\([a-z_.]*\) = .*/\1/p' /etc/postgresql/andromeda.conf.d/10-andromeda.conf)"
for key in $policy_keys; do
	file="$(sourcefile limits-2g "$key")"
	expect "policy $key from 10-andromeda.conf" "${file##*/}" 10-andromeda.conf
done
for key in autovacuum_work_mem io_workers temp_file_limit wal_buffers random_page_cost effective_io_concurrency \
	maintenance_io_concurrency io_combine_limit; do
	file="$(sourcefile limits-2g "$key")"
	expect "derived $key from andromeda-derived.conf" "${file##*/}" andromeda-derived.conf
done
expect 'reserved_connections' "$(sql limits-2g 'SHOW reserved_connections')" 3
expect 'idle_session_timeout' "$(sql limits-2g 'SHOW idle_session_timeout')" 1h
for key in synchronous_commit wal_log_hints autovacuum_vacuum_cost_delay; do
	expect "$key left at default" "$(sql limits-2g "SELECT source FROM pg_settings WHERE name = '$key'")" default
done

# 48MB differs from the 32MB the tuner computes for 2GB.
sql limits-2g "ALTER SYSTEM SET work_mem = '48MB'"
docker restart "$prefix-limits-2g" > /dev/null
wait_ready "$prefix-limits-2g"
expect 'ALTER SYSTEM work_mem survives re-tuning' "$(setting_mb limits-2g work_mem)" 48
file="$(sourcefile limits-2g work_mem)"
expect 'ALTER SYSTEM work_mem from postgresql.auto.conf' "${file##*/}" postgresql.auto.conf
# 512MB + 25 x 48MB x 2 + 10 x 32MB + 256MB.
expect 'the memory budget counts ALTER SYSTEM values' \
	"$(logs_mention limits-2g "$budget_warning of 3488MB exceed the 2048MB available")" yes

start limits-8g --memory 8g --cpus 4 --
expect '8g shared_buffers' "$(setting_mb limits-8g shared_buffers)" 2048
expect '8g max_parallel_workers' "$(sql limits-8g 'SHOW max_parallel_workers')" 4
# A 32nd of 2048MB shared_buffers, above the tuner's 16MB.
expect '8g wal_buffers' "$(setting_mb limits-8g wal_buffers)" 64
expect '8g logs no memory budget warning' "$(logs_mention limits-8g "$budget_warning")" no

start no-limits --
mem_total_mb="$(docker exec "$prefix-no-limits" awk '$1 == "MemTotal:" { print int($2 / 1024) }' /proc/meminfo)"
shared_kb="$(sql no-limits "SELECT pg_size_bytes(current_setting('shared_buffers')) / 1024")"
# Within 1MB: the tuner rounds a quarter of an odd MB count.
diff_kb=$(( shared_kb - mem_total_mb * 1024 / 4 ))
expect_true 'no limits shared_buffers is 25% of MemTotal' "$(( diff_kb >= -1024 && diff_kb <= 1024 ))"

start max-conns --memory 2g --cpus 2 -- -c max_connections=400
expect 'max_connections=400 written to postgresql.conf' \
	"$(docker exec "$prefix-max-conns" sh -c 'grep -c "^max_connections = 400" "$PGDATA/postgresql.conf"')" 1
docker exec -u postgres "$prefix-max-conns" sh -c \
	'timescaledb-tune --conf-path="$PGDATA/postgresql.conf" --out-path=/tmp/tuned.conf --memory=2048MB --cpus=2 --max-conns=400 --yes --quiet' > /dev/null
tuned_work_mem="$(docker exec "$prefix-max-conns" sed -n "s/^work_mem = \([0-9a-zA-Z]*\).*/\1/p" /tmp/tuned.conf)"
expect 'work_mem sized for 400 connections' \
	"$(sql max-conns "SELECT pg_size_bytes(current_setting('work_mem'))")" \
	"$(sql max-conns "SELECT pg_size_bytes('$tuned_work_mem')")"

start bg-workers --memory 2g --cpus 2 -e TS_TUNE_MAX_BG_WORKERS=100 --
expect 'TS_TUNE_MAX_BG_WORKERS sets timescaledb.max_background_workers' \
	"$(sql bg-workers 'SHOW timescaledb.max_background_workers')" 100
expect_true 'max_worker_processes covers background and parallel workers' \
	"$(sql bg-workers "SELECT (current_setting('max_worker_processes')::int >= 100 + current_setting('max_parallel_workers')::int)::int")"

start ts-tune-memory --memory 4g -e TS_TUNE_MEMORY=1GB --
expect 'TS_TUNE_MEMORY overrides the cgroup limit' "$(setting_mb ts-tune-memory shared_buffers)" 256

start small-volume --mount type=tmpfs,destination=/var/lib/postgresql,tmpfs-size=4294967296 --
expect_true 'max_wal_size below a 4GB volume' "$(( $(setting_mb small-volume max_wal_size) < 4096 ))"
data_kb="$(docker exec "$prefix-small-volume" sh -c 'df -Pk "$PGDATA" | awk "NR == 2 { print \$2 }"')"
expect 'temp_file_limit is 5% of the data volume' \
	"$(sql small-volume "SELECT setting FROM pg_settings WHERE name = 'temp_file_limit'")" $(( data_kb * 5 / 100 ))

# The tuner alone stays within budget (128kB work_mem for 400 connections in 1GB);
# a work_mem override undoes it.
start budget --memory 1g -- -c max_connections=400 -c work_mem=64MB
expect '1g with a 64MB work_mem override logs the memory budget warning' "$(logs_mention budget "$budget_warning")" yes

containers+=( "$prefix-volume" )
docker run -d --name "$prefix-volume" -e POSTGRES_PASSWORD=check -v "$volume:/var/lib/postgresql" \
	"${run_args[@]}" "$image" postgres > /dev/null
wait_ready "$prefix-volume"
docker stop "$prefix-volume" > /dev/null
conf_hash() {
	docker run --rm --entrypoint sh -v "$volume:/var/lib/postgresql" "${run_args[@]}" "$image" \
		-c 'sha256sum "$PGDATA/postgresql.conf"'
}
before="$(conf_hash)"
docker run --rm -v "$volume:/var/lib/postgresql" "${run_args[@]}" "$image" true
expect 'a command other than postgres leaves postgresql.conf unchanged' "$(conf_hash)" "$before"

if [ "$failures" -gt 0 ]; then
	printf '%s check(s) failed\n' "$failures"
	exit 1
fi
printf 'all checks passed\n'
