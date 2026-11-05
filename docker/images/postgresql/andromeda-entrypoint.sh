#!/usr/bin/env bash
# Runs the base image's entrypoint steps, names the superuser and the initial
# database andromeda, and tunes the server to the container's limits on every
# start before handing over to postgres.
set -Eeo pipefail

# The base image's entrypoint defines its steps as functions and runs none of
# them when sourced.
# shellcheck source=/dev/null
source /usr/local/bin/docker-entrypoint.sh

# Directory of the image's fixed policy files, included after the tuner's values.
ANDROMEDA_POLICY_DIR=/etc/postgresql/andromeda.conf.d

# usage: andromeda_main [command [args...]]
# The base image's start flow, with the tuning added and the initial database
# renamed. The base image's own flow cannot be reused: its re-exec as the
# postgres user names the base image's script, which would skip the tuning.
andromeda_main() {
	# A first argument that looks like a flag is a server option.
	if [ "${1:0:1}" = '-' ]; then
		set -- postgres "$@"
	fi

	if [ "$1" = 'postgres' ] && ! _pg_want_help "$@"; then
		andromeda_default_user
		docker_setup_env
		docker_create_db_directories
		if [ "$(id -u)" = '0' ]; then
			exec gosu postgres "${BASH_SOURCE[0]}" "$@"
		fi

		if [ -z "$DATABASE_ALREADY_EXISTS" ]; then
			docker_verify_minimum_env
			docker_error_old_databases

			# Fails before initdb when the init directory is unreadable, so no
			# half-initialised cluster is left behind.
			ls /docker-entrypoint-initdb.d/ > /dev/null

			docker_init_database_dir
			pg_setup_hba_conf "$@"

			# psql needs it when POSTGRES_INITDB_ARGS asks for password
			# authentication on local connections.
			export PGPASSWORD="${PGPASSWORD:-$POSTGRES_PASSWORD}"
			docker_temp_server_start "$@"

			andromeda_setup_db
			docker_process_init_files /docker-entrypoint-initdb.d/*

			docker_temp_server_stop
			unset PGPASSWORD

			printf '\nPostgreSQL init process complete; ready for start up.\n\n'
		else
			printf '\nPostgreSQL Database directory appears to contain a database; Skipping initialization\n\n'
		fi

		andromeda_tune "$@"
		# The postmaster keeps the environment it is started with, readable in
		# /proc/1/environ, so the password and the rest of the setup go first.
		unset "${!POSTGRES_@}"
	fi

	exec "$@"
}

# The superuser is andromeda unless POSTGRES_USER or POSTGRES_USER_FILE names
# another, and the initial database follows it. An image-level ENV would not
# do: the base image refuses a variable set alongside its _FILE form.
andromeda_default_user() {
	if [ -z "${POSTGRES_USER:-}" ] && [ -z "${POSTGRES_USER_FILE:-}" ]; then
		export POSTGRES_USER=andromeda
	fi
}

# usage: andromeda_setup_db
# Renames initdb's postgres database to POSTGRES_DB, so a new cluster holds only
# POSTGRES_DB and the templates. A POSTGRES_DB that already exists, postgres or
# template1 for example, is left as it is. Beside this session the temporary
# server runs only the TimescaleDB launcher, which holds no database connection,
# and postgres has no extension and so no scheduler: nothing is connected to it.
andromeda_setup_db() {
	local db="$POSTGRES_DB" exists
	exists="$(
		POSTGRES_DB='' docker_process_sql --dbname template1 --set db="$db" --tuples-only <<-'EOSQL'
			SELECT 1 FROM pg_database WHERE datname = :'db' ;
		EOSQL
	)"
	if [ -z "$exists" ]; then
		POSTGRES_DB='' docker_process_sql --dbname template1 --set db="$db" <<-'EOSQL'
			ALTER DATABASE postgres RENAME TO :"db" ;
		EOSQL
		printf '\n'
	fi
}

# usage: andromeda_tune postgres [server-args...]
# Runs timescaledb-tune on the container's limits rather than the host's, writes
# the values it does not derive, includes the fixed policy after both, and warns
# when the resulting memory ceilings exceed the container.
andromeda_tune() {
	shift
	local conf="$PGDATA/postgresql.conf"
	local begin='# --- andromeda overrides BEGIN ---'
	local end='# --- andromeda overrides END ---'

	# The tuner appends keys it finds missing after whatever ends the file,
	# so the include block is removed first and appended last.
	sed -i "/^${begin}\$/,/^${end}\$/d" "$conf"

	local limit_mb tune_mb cpus wal_disk_mb max_conns bg_workers
	limit_mb="$(andromeda_memory_mb)"
	tune_mb="$limit_mb"
	if [ -n "${TS_TUNE_MEMORY:-}" ]; then
		tune_mb="$(andromeda_size_mb TS_TUNE_MEMORY)"
	fi
	cpus="${TS_TUNE_NUM_CPUS:-}"
	if [ -z "$cpus" ]; then
		cpus="$(andromeda_cpus)"
	fi
	# The tuner gives max_wal_size about 60% of the disk it is told of and
	# min_wal_size half that, as if pg_wal had the disk to itself. A pg_wal on
	# the data volume shares it with the data, so the tuner is told of a quarter.
	wal_disk_mb=$(( $(andromeda_filesystem_kb "$PGDATA/pg_wal/") / 1024 ))
	if [ "$(stat -L -c %d "$PGDATA/pg_wal")" = "$(stat -c %d "$PGDATA")" ]; then
		wal_disk_mb=$(( wal_disk_mb / 4 ))
	fi
	max_conns="$(andromeda_command_line_max_connections "$@")"
	# The broker's default pools need about 150 connections.
	: "${max_conns:=${TS_TUNE_MAX_CONNS:-200}}"
	bg_workers="${TS_TUNE_MAX_BG_WORKERS:-64}"

	timescaledb-tune \
		--conf-path="$conf" \
		--memory="${tune_mb}MB" \
		--cpus="$cpus" \
		--max-conns="$max_conns" \
		--max-bg-workers="$bg_workers" \
		--wal-disk-size="${TS_TUNE_WAL_DISK_SIZE:-${wal_disk_mb}MB}" \
		--color=false \
		--yes

	andromeda_write_derived "$tune_mb" "$cpus" "$bg_workers" "$@"
	printf '%s\n' "$begin" "include_dir '$ANDROMEDA_POLICY_DIR'" "include 'andromeda-derived.conf'" "$end" >> "$conf"

	andromeda_check_memory_budget "$limit_mb" "$@"
	andromeda_check_shared_memory
}

# usage: andromeda_write_derived TUNE_MB CPUS BG_WORKERS [server-args...]
# Writes $PGDATA/andromeda-derived.conf with the hardware-dependent values the
# tuner does not compute. Rewritten on every start.
andromeda_write_derived() {
	local tune_mb="$1" cpus="$2" bg_workers="$3"
	shift 3
	local derived="$PGDATA/andromeda-derived.conf"

	local workers autovacuum_mb io_workers data_kb block_size shared_kb tuned_wal_kb wal_kb
	workers="$(postgres -D "$PGDATA" "$@" -C autovacuum_max_workers)"
	# Five percent of memory across every autovacuum worker, within 32MB..1GB each.
	autovacuum_mb=$(( tune_mb * 5 / 100 / workers ))
	if [ "$autovacuum_mb" -lt 32 ]; then
		autovacuum_mb=32
	elif [ "$autovacuum_mb" -gt 1024 ]; then
		autovacuum_mb=1024
	fi
	# PostgreSQL 18 accepts 1..32 I/O workers and defaults to 3.
	io_workers=$(( (cpus + 1) / 2 ))
	if [ "$io_workers" -lt 3 ]; then
		io_workers=3
	elif [ "$io_workers" -gt 32 ]; then
		io_workers=32
	fi
	# One spilling query fails instead of filling the data volume.
	data_kb="$(andromeda_filesystem_kb "$PGDATA")"
	# The tuner, like PostgreSQL's own sizing, stops at 16MB; WAL-heavy batch
	# writes then wait on WALWrite. A 32nd of shared_buffers, within 32MB and
	# 64MB, and never below the tuner's value. At 16MB a 2GB server ingests
	# about 3% slower with twice the WALWrite waiters; 32MB matches 64MB there.
	block_size="$(postgres -D "$PGDATA" "$@" -C block_size)"
	shared_kb=$(( $(postgres -D "$PGDATA" "$@" -C shared_buffers) * block_size / 1024 ))
	tuned_wal_kb=$(( $(postgres -D "$PGDATA" "$@" -C wal_buffers) * block_size / 1024 ))
	wal_kb=$(( shared_kb / 32 ))
	if [ "$wal_kb" -gt 65536 ]; then
		wal_kb=65536
	elif [ "$wal_kb" -lt 32768 ]; then
		wal_kb=32768
	fi
	if [ "$wal_kb" -lt "$tuned_wal_kb" ]; then
		wal_kb="$tuned_wal_kb"
	fi
	# Crash recovery replays up to max_wal_size, about 6 s per GB measured, so
	# it stops at 16GB. min_wal_size only keeps pg_wal from shrinking below it,
	# which spares creating segments after a quiet spell, so it stops at 2GB.
	local max_wal_mb min_wal_mb
	max_wal_mb="$(postgres -D "$PGDATA" "$@" -C max_wal_size)"
	min_wal_mb="$(postgres -D "$PGDATA" "$@" -C min_wal_size)"
	if [ "$max_wal_mb" -gt 16384 ]; then
		max_wal_mb=16384
	fi
	if [ "$min_wal_mb" -gt 2048 ]; then
		min_wal_mb=2048
	fi
	local work_mem_kb
	work_mem_kb="$(andromeda_work_mem_kb "$tune_mb" "$shared_kb" "$(( workers * autovacuum_mb * 1024 ))" "$@")"

	cat > "$derived" <<-EOF
		# Written by the entrypoint on every start; edits are lost. Override with ALTER SYSTEM.
		autovacuum_work_mem = ${autovacuum_mb}MB
		io_workers = ${io_workers}
		temp_file_limit = $(( data_kb * 5 / 100 ))kB
		wal_buffers = ${wal_kb}kB
		max_wal_size = ${max_wal_mb}MB
		min_wal_size = ${min_wal_mb}MB
		work_mem = ${work_mem_kb}kB
	EOF
	# At one CPU the tuner writes no worker settings, which leaves PostgreSQL's
	# 8 worker processes and TimescaleDB's 16 background workers. From two CPUs
	# up it sizes max_worker_processes as the background workers plus the CPUs
	# plus 3, which one CPU continues.
	if [ "$cpus" -eq 1 ]; then
		cat >> "$derived" <<-EOF
			max_worker_processes = $(( bg_workers + 4 ))
			timescaledb.max_background_workers = ${bg_workers}
			max_parallel_workers = 1
			max_parallel_workers_per_gather = 0
		EOF
	fi
}

# usage: andromeda_work_mem_kb TUNE_MB SHARED_KB AUTOVACUUM_KB [server-args...]
# The tuner sizes work_mem without hash_mem_multiplier or autovacuum, so its
# ceilings exceed memory at the default 200 connections in 2GB. Prints the
# memory left by shared_buffers, every autovacuum worker and
# maintenance_work_mem, over max_connections hash tables, never above the
# tuner's value and never below PostgreSQL's 64kB minimum.
andromeda_work_mem_kb() {
	local tune_mb="$1" shared_kb="$2" autovacuum_kb="$3"
	shift 3
	awk \
		-v tune_mb="$tune_mb" \
		-v shared_kb="$shared_kb" \
		-v autovacuum_kb="$autovacuum_kb" \
		-v maintenance_kb="$(postgres -D "$PGDATA" "$@" -C maintenance_work_mem)" \
		-v max_connections="$(postgres -D "$PGDATA" "$@" -C max_connections)" \
		-v hash_mem_multiplier="$(postgres -D "$PGDATA" "$@" -C hash_mem_multiplier)" \
		-v tuned_kb="$(postgres -D "$PGDATA" "$@" -C work_mem)" \
		'BEGIN {
			kb = int((tune_mb * 1024 - shared_kb - autovacuum_kb - maintenance_kb) / (max_connections * hash_mem_multiplier))
			if (kb > tuned_kb) kb = tuned_kb
			if (kb < 64) kb = 64
			printf "%d\n", kb
		}'
}

# usage: andromeda_check_memory_budget LIMIT_MB [server-args...]
# The sum is of ceilings, not allocations, so exceeding the limit warns.
andromeda_check_memory_budget() {
	local limit_mb="$1"
	shift
	local name
	local -A setting
	for name in block_size shared_buffers max_connections work_mem hash_mem_multiplier \
		autovacuum_max_workers autovacuum_work_mem maintenance_work_mem; do
		setting[$name]="$(postgres -D "$PGDATA" "$@" -C "$name")"
	done
	awk \
		-v limit_mb="$limit_mb" \
		-v block_size="${setting[block_size]}" \
		-v shared_buffers="${setting[shared_buffers]}" \
		-v max_connections="${setting[max_connections]}" \
		-v work_mem="${setting[work_mem]}" \
		-v hash_mem_multiplier="${setting[hash_mem_multiplier]}" \
		-v autovacuum_max_workers="${setting[autovacuum_max_workers]}" \
		-v autovacuum_work_mem="${setting[autovacuum_work_mem]}" \
		-v maintenance_work_mem="${setting[maintenance_work_mem]}" \
		'BEGIN {
			if (autovacuum_work_mem < 0) autovacuum_work_mem = maintenance_work_mem
			total_kb = shared_buffers * block_size / 1024 \
				+ max_connections * work_mem * hash_mem_multiplier \
				+ autovacuum_max_workers * autovacuum_work_mem \
				+ maintenance_work_mem
			if (total_kb / 1024 > limit_mb)
				printf "warning: memory ceilings of %dMB exceed the %dMB available: shared_buffers + max_connections x work_mem x hash_mem_multiplier + autovacuum_max_workers x autovacuum_work_mem + maintenance_work_mem\n", total_kb / 1024, limit_mb > "/dev/stderr"
		}'
}

# Dynamic shared memory lives in /dev/shm, which Docker sizes at 64MB unless
# told otherwise. The cumulative statistics grow with every tenant database and
# a parallel hash join allocates there too; running out fails the statement.
# Warns only: how much is needed depends on the number of tenant databases.
andromeda_check_shared_memory() {
	local shm_kb
	shm_kb="$(andromeda_filesystem_kb /dev/shm)"
	if [ "$shm_kb" -lt 1048576 ]; then
		printf >&2 'warning: /dev/shm holds %dMB, below the 1024MB recommended: dynamic shared memory (the cumulative statistics, parallel hash joins) grows with the tenant databases; raise it with docker run --shm-size or the Compose shm_size key\n' "$(( shm_kb / 1024 ))"
	fi
}

# Memory in MB: the cgroup v2 or v1 limit, or MemTotal when that is lower or there is none.
andromeda_memory_mb() {
	local total limit=''
	total="$(awk '$1 == "MemTotal:" { print int($2 / 1024) }' /proc/meminfo)"
	if [ -r /sys/fs/cgroup/memory.max ]; then
		limit="$(< /sys/fs/cgroup/memory.max)"
	elif [ -r /sys/fs/cgroup/memory/memory.limit_in_bytes ]; then
		limit="$(< /sys/fs/cgroup/memory/memory.limit_in_bytes)"
	fi
	# cgroup v1 spells "unlimited" as a near-2^63 byte count, which fits a signed 64-bit integer.
	if [ -n "$limit" ] && [ "$limit" != max ] && [ $(( limit / 1048576 )) -lt "$total" ]; then
		total=$(( limit / 1048576 ))
	fi
	printf '%s\n' "$total"
}

# CPUs: the cgroup v2 or v1 quota over its period, rounded up, capped by the CPUs this process may run on.
andromeda_cpus() {
	local cpus quota='' period=''
	cpus="$(nproc)"
	if [ -r /sys/fs/cgroup/cpu.max ]; then
		read -r quota period < /sys/fs/cgroup/cpu.max
	elif [ -r /sys/fs/cgroup/cpu/cpu.cfs_quota_us ]; then
		quota="$(< /sys/fs/cgroup/cpu/cpu.cfs_quota_us)"
		period="$(< /sys/fs/cgroup/cpu/cpu.cfs_period_us)"
	fi
	if [ -n "$quota" ] && [ "$quota" != max ] && [ "$quota" -gt 0 ]; then
		quota=$(( (quota + period - 1) / period ))
		if [ "$quota" -lt "$cpus" ]; then
			cpus="$quota"
		fi
	fi
	printf '%s\n' "$cpus"
}

# usage: andromeda_size_mb VAR
# Converts the PostgreSQL-format size in $VAR (for example 4GB) to whole MB.
andromeda_size_mb() {
	local var="$1"
	if [[ ! "${!var}" =~ ^([0-9]+)(kB|MB|GB|TB)$ ]]; then
		printf >&2 'error: %s must be a size such as 4GB, got %s\n' "$var" "${!var}"
		exit 1
	fi
	local value="${BASH_REMATCH[1]}"
	case "${BASH_REMATCH[2]}" in
		kB) printf '%s\n' $(( value / 1024 )) ;;
		MB) printf '%s\n' "$value" ;;
		GB) printf '%s\n' $(( value * 1024 )) ;;
		TB) printf '%s\n' $(( value * 1048576 )) ;;
	esac
}

# usage: andromeda_filesystem_kb PATH
# Size in kB of the filesystem holding PATH; a trailing slash follows a symlinked pg_wal.
andromeda_filesystem_kb() {
	df -Pk "$1" | awk 'NR == 2 { print $2 }'
}

# usage: andromeda_command_line_max_connections [server-args...]
# Prints the max_connections the server arguments set, the last one winning as
# it does for postgres, or nothing when they set none.
andromeda_command_line_max_connections() {
	local arg value='' pending=''
	for arg; do
		case "$pending" in
			c) arg="-c$arg" ;;
			N) arg="-N$arg" ;;
		esac
		pending=
		case "$arg" in
			-c | -N) pending="${arg#-}" ;;
			-cmax_connections=* | --max_connections=* | --max-connections=*) value="${arg#*=}" ;;
			-N?*) value="${arg#-N}" ;;
		esac
	done
	printf '%s' "$value"
}

andromeda_main "$@"
