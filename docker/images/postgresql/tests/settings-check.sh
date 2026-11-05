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
mounts="$(mktemp -d)"
chmod 755 "$mounts"

cleanup() {
	if [ "${#containers[@]}" -gt 0 ]; then
		docker rm -f "${containers[@]}" > /dev/null 2>&1 || :
	fi
	docker volume rm -f "$volume" > /dev/null 2>&1 || :
	rm -rf "$mounts"
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

# usage: sql_in NAME ROLE DATABASE QUERY
sql_in() {
	docker exec -u postgres "$prefix-$1" psql -U "$2" -d "$3" -XAtq -c "$4"
}

# usage: sql_as NAME ROLE QUERY
# Connects to the role's own database, as psql does with no -d.
sql_as() {
	docker exec -u postgres "$prefix-$1" psql -U "$2" -XAtq -c "$3"
}

# usage: sql NAME QUERY
sql() {
	sql_as "$1" andromeda "$2"
}

# usage: databases NAME ROLE DATABASE; prints every database name, comma-separated.
databases() {
	sql_in "$1" "$2" "$3" "SELECT string_agg(datname, ',' ORDER BY datname) FROM pg_database"
}

# usage: tuned NAME SETTING [timescaledb-tune-args...]
# Prints in base units what timescaledb-tune recommends for SETTING with the
# given arguments, the way the entrypoint runs it.
tuned() {
	local name="$1" setting="$2"
	shift 2
	docker exec -u postgres "$prefix-$name" sh -c \
		'timescaledb-tune --conf-path="$PGDATA/postgresql.conf" --out-path=/tmp/tuned.conf --yes --quiet --color=false "$@" > /dev/null 2>&1' \
		sh "$@"
	local value
	value="$(docker exec "$prefix-$name" sed -n "s/^$setting = \([0-9a-zA-Z]*\).*/\1/p" /tmp/tuned.conf)"
	sql "$name" "SELECT pg_size_bytes('$value') / 1048576"
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
expect '2g max_connections defaults to 200' "$(sql limits-2g 'SHOW max_connections')" 200
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
# The tuner's 4MB for 200 connections would make the ceilings 2688MB
# (512MB + 200 x 4MB x 2 + 10 x 32MB + 256MB). What the rest leaves, over 200
# connections' hash tables: (2048MB - 512MB - 10 x 32MB - 256MB) / (200 x 2).
expect_true '2g work_mem below the tuner'"'"'s' \
	"$(( $(sql limits-2g "SELECT pg_size_bytes(current_setting('work_mem'))") < $(tuned limits-2g work_mem --memory=2048MB --cpus=2 --max-conns=200) * 1048576 ))"
expect '2g work_mem fits the memory' "$(sql limits-2g "SELECT pg_size_bytes(current_setting('work_mem')) / 1024")" 2457
expect '2g logs no memory budget warning' "$(logs_mention limits-2g "$budget_warning")" no
expect '2g postmaster environment holds no POSTGRES_ variable' \
	"$(docker exec -u postgres "$prefix-limits-2g" sh -c "tr '\\0' '\\n' < /proc/1/environ | grep -c '^POSTGRES_'" || :)" 0
# Docker's default /dev/shm is 64MB.
expect '2g logs the /dev/shm warning' "$(logs_mention limits-2g 'warning: /dev/shm holds 64MB')" yes

expect 'andromeda is a superuser' "$(sql limits-2g "SELECT rolsuper FROM pg_roles WHERE rolname = 'andromeda'")" t
expect 'no postgres role' "$(sql limits-2g "SELECT count(*) FROM pg_roles WHERE rolname = 'postgres'")" 0
expect 'psql -U andromeda lands in andromeda' "$(sql limits-2g 'SELECT current_database()')" andromeda
expect 'postgres is renamed to andromeda' "$(databases limits-2g andromeda andromeda)" andromeda,template0,template1

# TimescaleDB registers its settings only in a database holding the extension.
sql limits-2g 'CREATE EXTENSION IF NOT EXISTS timescaledb' > /dev/null
if docker exec "$prefix-limits-2g" sh -c 'ls "$(pg_config --pkglibdir)" | grep -q "^timescaledb-tsl-"'; then
	license=timescale
else
	license=apache
fi
expect "timescaledb.license matches the installed edition" "$(sql limits-2g 'SHOW timescaledb.license')" "$license"
expect 'pg_stat_statements.max' "$(sql limits-2g 'SHOW pg_stat_statements.max')" 20000
expect 'autovacuum_naptime overrides the tuner' "$(sql limits-2g 'SHOW autovacuum_naptime')" 1min
policy_keys="$(docker exec "$prefix-limits-2g" sed -n 's/^\([a-z_.]*\) = .*/\1/p' /etc/postgresql/andromeda.conf.d/10-andromeda.conf)"
for key in $policy_keys; do
	file="$(sourcefile limits-2g "$key")"
	expect "policy $key from 10-andromeda.conf" "${file##*/}" 10-andromeda.conf
done
for key in autovacuum_work_mem io_workers temp_file_limit wal_buffers max_wal_size min_wal_size work_mem; do
	file="$(sourcefile limits-2g "$key")"
	expect "derived $key from andromeda-derived.conf" "${file##*/}" andromeda-derived.conf
done
expect 'io_combine_limit left at default' "$(sql limits-2g "SELECT source FROM pg_settings WHERE name = 'io_combine_limit'")" default
expect 'reserved_connections' "$(sql limits-2g 'SHOW reserved_connections')" 3
expect 'idle_session_timeout' "$(sql limits-2g 'SHOW idle_session_timeout')" 1h
for key in synchronous_commit wal_log_hints autovacuum_vacuum_cost_delay; do
	expect "$key left at default" "$(sql limits-2g "SELECT source FROM pg_settings WHERE name = '$key'")" default
done

# 48MB differs from the work_mem the entrypoint derives for 2GB.
sql limits-2g "ALTER SYSTEM SET work_mem = '48MB'"
docker restart "$prefix-limits-2g" > /dev/null
wait_ready "$prefix-limits-2g"
expect 'ALTER SYSTEM work_mem survives re-tuning' "$(setting_mb limits-2g work_mem)" 48
file="$(sourcefile limits-2g work_mem)"
expect 'ALTER SYSTEM work_mem from postgresql.auto.conf' "${file##*/}" postgresql.auto.conf
# 512MB + 200 x 48MB x 2 + 10 x 32MB + 256MB.
expect 'the memory budget counts ALTER SYSTEM values' \
	"$(logs_mention limits-2g "$budget_warning of 20288MB exceed the 2048MB available")" yes

start limits-8g --memory 8g --cpus 4 --
expect '8g shared_buffers' "$(setting_mb limits-8g shared_buffers)" 2048
expect '8g max_connections defaults to 200' "$(sql limits-8g 'SHOW max_connections')" 200
# 8192MB - 2048MB - 10 x 40MB - 1024MB leaves more than the tuner's value for 200 connections.
expect '8g work_mem is the tuner'"'"'s' "$(setting_mb limits-8g work_mem)" \
	"$(tuned limits-8g work_mem --memory=8192MB --cpus=4 --max-conns=200)"
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
# (2048MB - 512MB - 10 x 32MB - 256MB) / (400 x 2), below the tuner's value.
expect 'work_mem sized for 400 connections' "$(sql max-conns "SELECT pg_size_bytes(current_setting('work_mem')) / 1024")" 1228
expect_true 'work_mem for 400 connections below the tuner'"'"'s' \
	"$(( 1228 < $(tuned max-conns work_mem --memory=2048MB --cpus=2 --max-conns=400) * 1024 ))"

start bg-workers --memory 2g --cpus 2 -e TS_TUNE_MAX_BG_WORKERS=100 --
expect 'TS_TUNE_MAX_BG_WORKERS sets timescaledb.max_background_workers' \
	"$(sql bg-workers 'SHOW timescaledb.max_background_workers')" 100
expect_true 'max_worker_processes covers background and parallel workers' \
	"$(sql bg-workers "SELECT (current_setting('max_worker_processes')::int >= 100 + current_setting('max_parallel_workers')::int)::int")"

start ts-tune-memory --memory 4g -e TS_TUNE_MEMORY=1GB --
expect 'TS_TUNE_MEMORY overrides the cgroup limit' "$(setting_mb ts-tune-memory shared_buffers)" 256

start one-cpu --memory 1g --cpus 1 --
# The tuner writes no worker settings at one CPU; at two and more it writes
# the background workers plus the CPUs plus 3.
expect '1 CPU max_worker_processes' "$(sql one-cpu 'SHOW max_worker_processes')" 68
expect '1 CPU timescaledb.max_background_workers' "$(sql one-cpu 'SHOW timescaledb.max_background_workers')" 64
expect '1 CPU max_parallel_workers' "$(sql one-cpu 'SHOW max_parallel_workers')" 1
expect '1 CPU max_parallel_workers_per_gather' "$(sql one-cpu 'SHOW max_parallel_workers_per_gather')" 0
for key in max_worker_processes timescaledb.max_background_workers max_parallel_workers max_parallel_workers_per_gather; do
	file="$(sourcefile one-cpu "$key")"
	expect "1 CPU $key from andromeda-derived.conf" "${file##*/}" andromeda-derived.conf
done

# A 20GB volume holding pg_wal too: the tuner is told of a quarter of it.
start shared-wal --mount type=tmpfs,destination=/var/lib/postgresql,tmpfs-size=21474836480 --
expect 'shared pg_wal max_wal_size from a quarter of the volume' "$(setting_mb shared-wal max_wal_size)" \
	"$(tuned shared-wal max_wal_size --memory=2048MB --cpus=2 --wal-disk-size=5120MB)"
expect 'shared pg_wal min_wal_size from a quarter of the volume' "$(setting_mb shared-wal min_wal_size)" \
	"$(tuned shared-wal min_wal_size --memory=2048MB --cpus=2 --wal-disk-size=5120MB)"

start dedicated-wal --mount type=tmpfs,destination=/var/lib/postgresql,tmpfs-size=21474836480 \
	--mount type=tmpfs,destination=/var/lib/postgresql/wal,tmpfs-size=21474836480 \
	-e POSTGRES_INITDB_WALDIR=/var/lib/postgresql/wal --
expect 'dedicated pg_wal max_wal_size from the whole volume' "$(setting_mb dedicated-wal max_wal_size)" \
	"$(tuned dedicated-wal max_wal_size --memory=2048MB --cpus=2 --wal-disk-size=20480MB)"
expect 'dedicated pg_wal min_wal_size capped at 2GB' "$(setting_mb dedicated-wal min_wal_size)" 2048

start small-volume --mount type=tmpfs,destination=/var/lib/postgresql,tmpfs-size=4294967296 --
expect_true 'max_wal_size below a quarter of a 4GB volume' "$(( $(setting_mb small-volume max_wal_size) < 1024 ))"
data_kb="$(docker exec "$prefix-small-volume" sh -c 'df -Pk "$PGDATA" | awk "NR == 2 { print \$2 }"')"
expect 'temp_file_limit is 5% of the data volume' \
	"$(sql small-volume "SELECT setting FROM pg_settings WHERE name = 'temp_file_limit'")" $(( data_kb * 5 / 100 ))

# The tuner derives about 39GB and 19GB from a dedicated 64GB WAL volume;
# tmpfs reserves nothing up front.
start large-volume --mount type=tmpfs,destination=/var/lib/postgresql/wal,tmpfs-size=68719476736 \
	-e POSTGRES_INITDB_WALDIR=/var/lib/postgresql/wal --
expect 'max_wal_size capped at 16GB' "$(setting_mb large-volume max_wal_size)" 16384
expect 'min_wal_size capped at 2GB' "$(setting_mb large-volume min_wal_size)" 2048

start shm-1g --shm-size 1g --
expect 'a 1GB /dev/shm logs no /dev/shm warning' "$(logs_mention shm-1g 'warning: /dev/shm')" no

start custom-role -e POSTGRES_USER=custom --
expect 'POSTGRES_USER names the superuser' "$(sql_as custom-role custom "SELECT rolsuper FROM pg_roles WHERE rolname = 'custom'")" t
expect 'POSTGRES_USER names the initial database' "$(databases custom-role custom custom)" custom,template0,template1
expect 'POSTGRES_USER leaves no andromeda role' \
	"$(sql_as custom-role custom "SELECT count(*) FROM pg_roles WHERE rolname = 'andromeda'")" 0

start keep-postgres -e POSTGRES_DB=postgres --
expect 'POSTGRES_DB=postgres keeps the postgres database' "$(databases keep-postgres andromeda postgres)" postgres,template0,template1

mkdir -p "$mounts/initdb"
printf 'CREATE TABLE init_probe ();\n' > "$mounts/initdb/10-probe.sql"
chmod -R a+rX "$mounts/initdb"
start init-scripts -v "$mounts/initdb:/docker-entrypoint-initdb.d:ro" --
expect 'init scripts run in andromeda' "$(sql init-scripts "SELECT to_regclass('public.init_probe') IS NOT NULL")" t

printf 'fromfile' > "$mounts/user"
chmod a+r "$mounts/user"
start user-file -v "$mounts/user:/run/user:ro" -e POSTGRES_USER_FILE=/run/user --
expect 'POSTGRES_USER_FILE names the superuser' \
	"$(sql_as user-file fromfile "SELECT count(*) FROM pg_roles WHERE rolname = 'fromfile' AND rolsuper")" 1

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
