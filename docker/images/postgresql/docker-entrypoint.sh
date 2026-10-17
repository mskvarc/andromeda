#!/usr/bin/env bash
# docker-library/postgres entrypoint + timescaledb-tune fed the container's limits (andromeda_tune).
set -Eeo pipefail
# TODO swap to -Eeuo pipefail above (after handling all potentially-unset variables)

# usage: file_env VAR [DEFAULT]
#    ie: file_env 'XYZ_DB_PASSWORD' 'example'
# (will allow for "$XYZ_DB_PASSWORD_FILE" to fill in the value of
#  "$XYZ_DB_PASSWORD" from a file, especially for Docker's secrets feature)
file_env() {
	local var="$1"
	local fileVar="${var}_FILE"
	local def="${2:-}"
	if [ "${!var:-}" ] && [ "${!fileVar:-}" ]; then
		printf >&2 'error: both %s and %s are set (but are exclusive)\n' "$var" "$fileVar"
		exit 1
	fi
	local val="$def"
	if [ "${!var:-}" ]; then
		val="${!var}"
	elif [ "${!fileVar:-}" ]; then
		val="$(< "${!fileVar}")"
	fi
	export "$var"="$val"
	unset "$fileVar"
}

# check to see if this file is being run or sourced from another script
_is_sourced() {
	# https://unix.stackexchange.com/a/215279
	[ "${#FUNCNAME[@]}" -ge 2 ] \
		&& [ "${FUNCNAME[0]}" = '_is_sourced' ] \
		&& [ "${FUNCNAME[1]}" = 'source' ]
}

# used to create initial postgres directories and if run as root, ensure ownership to the "postgres" user
docker_create_db_directories() {
	local user; user="$(id -u)"

	mkdir -p "$PGDATA"
	# ignore failure since there are cases where we can't chmod (and PostgreSQL might fail later anyhow - it's picky about permissions of this directory)
	chmod 00700 "$PGDATA" || :

	# ignore failure since it will be fine when using the image provided directory; see also https://github.com/docker-library/postgres/pull/289
	mkdir -p /var/run/postgresql || :
	chmod 03775 /var/run/postgresql || :

	# Create the transaction log directory before initdb is run so the directory is owned by the correct user
	if [ -n "${POSTGRES_INITDB_WALDIR:-}" ]; then
		mkdir -p "$POSTGRES_INITDB_WALDIR"
		if [ "$user" = '0' ]; then
			find "$POSTGRES_INITDB_WALDIR" \! -user postgres -exec chown postgres '{}' +
		fi
		chmod 700 "$POSTGRES_INITDB_WALDIR"
	fi

	# allow the container to be started with `--user`
	if [ "$user" = '0' ]; then
		find "$PGDATA" \! -user postgres -exec chown postgres '{}' +
		find /var/run/postgresql \! -user postgres -exec chown postgres '{}' +
	fi
}

# initialize empty PGDATA directory with new database via 'initdb'
# arguments to `initdb` can be passed via POSTGRES_INITDB_ARGS or as arguments to this function
# `initdb` automatically creates the "postgres", "template0", and "template1" dbnames
# this is also where the database user is created, specified by `POSTGRES_USER` env
docker_init_database_dir() {
	# "initdb" is particular about the current user existing in "/etc/passwd", so we use "nss_wrapper" to fake that if necessary
	# see https://github.com/docker-library/postgres/pull/253, https://github.com/docker-library/postgres/issues/359, https://cwrap.org/nss_wrapper.html
	local uid; uid="$(id -u)"
	if ! getent passwd "$uid" &> /dev/null; then
		# see if we can find a suitable "libnss_wrapper.so" (https://salsa.debian.org/sssd-team/nss-wrapper/-/commit/b9925a653a54e24d09d9b498a2d913729f7abb15)
		local wrapper
		for wrapper in {/usr,}/lib{/*,}/libnss_wrapper.so; do
			if [ -s "$wrapper" ]; then
				NSS_WRAPPER_PASSWD="$(mktemp)"
				NSS_WRAPPER_GROUP="$(mktemp)"
				export LD_PRELOAD="$wrapper" NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP
				local gid; gid="$(id -g)"
				printf 'postgres:x:%s:%s:PostgreSQL:%s:/bin/false\n' "$uid" "$gid" "$PGDATA" > "$NSS_WRAPPER_PASSWD"
				printf 'postgres:x:%s:\n' "$gid" > "$NSS_WRAPPER_GROUP"
				break
			fi
		done
	fi

	if [ -n "${POSTGRES_INITDB_WALDIR:-}" ]; then
		set -- --waldir "$POSTGRES_INITDB_WALDIR" "$@"
	fi

	# --pwfile refuses to handle a properly-empty file (hence the "\n"): https://github.com/docker-library/postgres/issues/1025
	eval 'initdb --username="$POSTGRES_USER" --pwfile=<(printf "%s\n" "$POSTGRES_PASSWORD") '"$POSTGRES_INITDB_ARGS"' "$@"'

	# unset/cleanup "nss_wrapper" bits
	if [[ "${LD_PRELOAD:-}" == */libnss_wrapper.so ]]; then
		rm -f "$NSS_WRAPPER_PASSWD" "$NSS_WRAPPER_GROUP"
		unset LD_PRELOAD NSS_WRAPPER_PASSWD NSS_WRAPPER_GROUP
	fi
}

# print large warning if POSTGRES_PASSWORD is long
# error if both POSTGRES_PASSWORD is empty and POSTGRES_HOST_AUTH_METHOD is not 'trust'
# print large warning if POSTGRES_HOST_AUTH_METHOD is set to 'trust'
# assumes database is not set up, ie: [ -z "$DATABASE_ALREADY_EXISTS" ]
docker_verify_minimum_env() {
	if [ -z "$POSTGRES_PASSWORD" ] && [ 'trust' != "$POSTGRES_HOST_AUTH_METHOD" ]; then
		# The - option suppresses leading tabs but *not* spaces. :)
		cat >&2 <<-'EOE'
			Error: Database is uninitialized and superuser password is not specified.
			       You must specify POSTGRES_PASSWORD to a non-empty value for the
			       superuser. For example, "-e POSTGRES_PASSWORD=password" on "docker run".

			       You may also use "POSTGRES_HOST_AUTH_METHOD=trust" to allow all
			       connections without a password. This is *not* recommended.

			       See PostgreSQL documentation about "trust":
			       https://www.postgresql.org/docs/current/auth-trust.html
		EOE
		exit 1
	fi
	if [ 'trust' = "$POSTGRES_HOST_AUTH_METHOD" ]; then
		cat >&2 <<-'EOWARN'
			********************************************************************************
			WARNING: POSTGRES_HOST_AUTH_METHOD has been set to "trust". This will allow
			         anyone with access to the Postgres port to access your database without
			         a password, even if POSTGRES_PASSWORD is set. See PostgreSQL
			         documentation about "trust":
			         https://www.postgresql.org/docs/current/auth-trust.html
			         In Docker's default configuration, this is effectively any other
			         container on the same system.

			         It is not recommended to use POSTGRES_HOST_AUTH_METHOD=trust. Replace
			         it with "-e POSTGRES_PASSWORD=password" instead to set a password in
			         "docker run".
			********************************************************************************
		EOWARN
	fi
}
# similar to the above, but errors if there are any "old" databases detected (usually due to upgrades without pg_upgrade)
docker_error_old_databases() {
	if [ -n "${OLD_DATABASES[0]:-}" ]; then
		cat >&2 <<-EOE
			Error: in 18+, these Docker images are configured to store database data in a
			       format which is compatible with "pg_ctlcluster" (specifically, using
			       major-version-specific directory names).  This better reflects how
			       PostgreSQL itself works, and how upgrades are to be performed.

			       See also https://github.com/docker-library/postgres/pull/1259

			       Counter to that, there appears to be PostgreSQL data in:
			         ${OLD_DATABASES[*]}

			       This is usually the result of upgrading the Docker image without
			       upgrading the underlying database using "pg_upgrade" (which requires both
			       versions).

			       The suggested container configuration for 18+ is to place a single mount
			       at /var/lib/postgresql which will then place PostgreSQL data in a
			       subdirectory, allowing usage of "pg_upgrade --link" without mount point
			       boundary issues.

			       See https://github.com/docker-library/postgres/issues/37 for a (long)
			       discussion around this process, and suggestions for how to do so.
		EOE
		exit 1
	fi
}

# usage: docker_process_init_files [file [file [...]]]
#    ie: docker_process_init_files /always-initdb.d/*
# process initializer files, based on file extensions and permissions
docker_process_init_files() {
	# psql here for backwards compatibility "${psql[@]}"
	# Read by init scripts that are sourced below, which shellcheck cannot see.
	# shellcheck disable=SC2034
	psql=( docker_process_sql )

	printf '\n'
	local f
	for f; do
		case "$f" in
			*.sh)
				# https://github.com/docker-library/postgres/issues/450#issuecomment-393167936
				# https://github.com/docker-library/postgres/pull/452
				if [ -x "$f" ]; then
					printf '%s: running %s\n' "$0" "$f"
					"$f"
				else
					printf '%s: sourcing %s\n' "$0" "$f"
					# An init script mounted at run time; there is nothing to follow.
					# shellcheck source=/dev/null
					. "$f"
				fi
				;;
			*.sql)     printf '%s: running %s\n' "$0" "$f"; docker_process_sql -f "$f"; printf '\n' ;;
			*.sql.gz)  printf '%s: running %s\n' "$0" "$f"; gunzip -c "$f" | docker_process_sql; printf '\n' ;;
			*.sql.xz)  printf '%s: running %s\n' "$0" "$f"; xzcat "$f" | docker_process_sql; printf '\n' ;;
			*.sql.zst) printf '%s: running %s\n' "$0" "$f"; zstd -dc "$f" | docker_process_sql; printf '\n' ;;
			*)         printf '%s: ignoring %s\n' "$0" "$f" ;;
		esac
		printf '\n'
	done
}

# Execute sql script, passed via stdin (or -f flag of pqsl)
# usage: docker_process_sql [psql-cli-args]
#    ie: docker_process_sql --dbname=mydb <<<'INSERT ...'
#    ie: docker_process_sql -f my-file.sql
#    ie: docker_process_sql <my-file.sql
docker_process_sql() {
	local query_runner=( psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --no-password --no-psqlrc )
	if [ -n "$POSTGRES_DB" ]; then
		query_runner+=( --dbname "$POSTGRES_DB" )
	fi

	PGHOST='' PGHOSTADDR='' "${query_runner[@]}" "$@"
}

# create initial database
# uses environment variables for input: POSTGRES_DB
docker_setup_db() {
	local db="$POSTGRES_DB"
	local dbAlreadyExists
	dbAlreadyExists="$(
		POSTGRES_DB='' docker_process_sql --dbname postgres --set db="$db" --tuples-only <<-'EOSQL'
			SELECT 1 FROM pg_database WHERE datname = :'db' ;
		EOSQL
	)"
	if [ -z "$dbAlreadyExists" ]; then
		POSTGRES_DB='' docker_process_sql --dbname postgres --set db="$db" <<-'EOSQL'
			CREATE DATABASE :"db" ;
		EOSQL
		printf '\n'
	fi
}

# Loads various settings that are used elsewhere in the script
# This should be called before any other functions
docker_setup_env() {
	file_env 'POSTGRES_PASSWORD'

	file_env 'POSTGRES_USER' 'postgres'
	file_env 'POSTGRES_DB' "$POSTGRES_USER"
	file_env 'POSTGRES_INITDB_ARGS'
	: "${POSTGRES_HOST_AUTH_METHOD:=}"

	declare -g DATABASE_ALREADY_EXISTS
	: "${DATABASE_ALREADY_EXISTS:=}"
	declare -ag OLD_DATABASES=()
	# look specifically for PG_VERSION, as it is expected in the DB dir
	if [ -s "$PGDATA/PG_VERSION" ]; then
		DATABASE_ALREADY_EXISTS='true'
	elif [ "$PGDATA" = "/var/lib/postgresql/$PG_MAJOR/docker" ]; then
		# https://github.com/docker-library/postgres/pull/1259
		for d in /var/lib/postgresql /var/lib/postgresql/data /var/lib/postgresql/*/docker; do
			if [ -s "$d/PG_VERSION" ]; then
				OLD_DATABASES+=( "$d" )
			fi
		done
		if [ "${#OLD_DATABASES[@]}" -eq 0 ] && [ "$PG_MAJOR" -ge 18 ] && mountpoint -q /var/lib/postgresql/data; then
			OLD_DATABASES+=( '/var/lib/postgresql/data (unused mount/volume)' )
		fi
	fi
}

# append POSTGRES_HOST_AUTH_METHOD to pg_hba.conf for "host" connections
# all arguments will be passed along as arguments to `postgres` for getting the value of 'password_encryption'
pg_setup_hba_conf() {
	# default authentication method is md5 on versions before 14
	# https://www.postgresql.org/about/news/postgresql-14-released-2318/
	if [ "$1" = 'postgres' ]; then
		shift
	fi
	local auth
	# check the default/configured encryption and use that as the auth method
	auth="$(postgres -C password_encryption "$@")"
	: "${POSTGRES_HOST_AUTH_METHOD:=$auth}"
	{
		printf '\n'
		if [ 'trust' = "$POSTGRES_HOST_AUTH_METHOD" ]; then
			printf '# warning trust is enabled for all connections\n'
			printf '# see https://www.postgresql.org/docs/17/auth-trust.html\n'
		fi
		printf 'host all all all %s\n' "$POSTGRES_HOST_AUTH_METHOD"
	} >> "$PGDATA/pg_hba.conf"
}

# start socket-only postgresql server for setting up or running scripts
# all arguments will be passed along as arguments to `postgres` (via pg_ctl)
docker_temp_server_start() {
	if [ "$1" = 'postgres' ]; then
		shift
	fi

	# internal start of server in order to allow setup using psql client
	# does not listen on external TCP/IP and waits until start finishes
	set -- "$@" -c listen_addresses='' -p "${PGPORT:-5432}"

	# unset NOTIFY_SOCKET so the temporary server doesn't prematurely notify
	# any process supervisor.
	NOTIFY_SOCKET='' \
	PGUSER="${PGUSER:-$POSTGRES_USER}" \
	pg_ctl -D "$PGDATA" \
		-o "$(printf '%q ' "$@")" \
		-w start
}

# stop postgresql server after done setting up user and running scripts
docker_temp_server_stop() {
	PGUSER="${PGUSER:-postgres}" \
	pg_ctl -D "$PGDATA" -m fast -w stop
}

# check arguments for an option that would cause postgres to stop
# return true if there is one
_pg_want_help() {
	local arg
	for arg; do
		case "$arg" in
			# postgres --help | grep 'then exit'
			# leaving out -C on purpose since it always fails and is unhelpful:
			# postgres: could not access the server configuration file "/var/lib/postgresql/data/postgresql.conf": No such file or directory
			-'?'|--help|--describe-config|-V|--version)
				return 0
				;;
		esac
	done
	return 1
}

_main() {
	# if first arg looks like a flag, assume we want to run postgres server
	if [ "${1:0:1}" = '-' ]; then
		set -- postgres "$@"
	fi

	if [ "$1" = 'postgres' ] && ! _pg_want_help "$@"; then
		docker_setup_env
		# setup data directories and permissions (when run as root)
		docker_create_db_directories
		if [ "$(id -u)" = '0' ]; then
			# then restart script as postgres user
			exec gosu postgres "${BASH_SOURCE[0]}" "$@"
		fi

		# only run initialization on an empty data directory
		if [ -z "$DATABASE_ALREADY_EXISTS" ]; then
			docker_verify_minimum_env
			docker_error_old_databases

			# check dir permissions to reduce likelihood of half-initialized database
			ls /docker-entrypoint-initdb.d/ > /dev/null

			docker_init_database_dir
			pg_setup_hba_conf "$@"

			# PGPASSWORD is required for psql when authentication is required for 'local' connections via pg_hba.conf and is otherwise harmless
			# e.g. when '--auth=md5' or '--auth-local=md5' is used in POSTGRES_INITDB_ARGS
			export PGPASSWORD="${PGPASSWORD:-$POSTGRES_PASSWORD}"
			docker_temp_server_start "$@"

			docker_setup_db
			docker_process_init_files /docker-entrypoint-initdb.d/*

			docker_temp_server_stop
			unset PGPASSWORD

			cat <<-'EOM'

				PostgreSQL init process complete; ready for start up.

			EOM
		else
			cat <<-'EOM'

				PostgreSQL Database directory appears to contain a database; Skipping initialization

			EOM
		fi

		andromeda_tune "$@"
	fi

	exec "$@"
}

# Directory of the image's fixed policy files, included after the tuner's values.
ANDROMEDA_POLICY_DIR=/etc/postgresql/andromeda.conf.d

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

	local limit_mb tune_mb cpus wal_kb max_conns
	limit_mb="$(andromeda_memory_mb)"
	tune_mb="$limit_mb"
	if [ -n "${TS_TUNE_MEMORY:-}" ]; then
		tune_mb="$(andromeda_size_mb TS_TUNE_MEMORY)"
	fi
	cpus="${TS_TUNE_NUM_CPUS:-}"
	if [ -z "$cpus" ]; then
		cpus="$(andromeda_cpus)"
	fi
	wal_kb="$(andromeda_filesystem_kb "$PGDATA/pg_wal/")"
	max_conns="$(andromeda_command_line_max_connections "$@")"
	: "${max_conns:=${TS_TUNE_MAX_CONNS:-}}"

	local tune_args=(
		--conf-path="$conf"
		--memory="${tune_mb}MB"
		--cpus="$cpus"
		--max-bg-workers="${TS_TUNE_MAX_BG_WORKERS:-64}"
		--wal-disk-size="${TS_TUNE_WAL_DISK_SIZE:-$(( wal_kb / 1024 ))MB}"
		--color=false
		--yes
	)
	# Without --max-conns the tuner recommends max_connections from memory.
	if [ -n "$max_conns" ]; then
		tune_args+=( --max-conns="$max_conns" )
	fi
	timescaledb-tune "${tune_args[@]}"

	andromeda_write_derived "$tune_mb" "$cpus" "$@"
	printf '%s\n' "$begin" "include_dir '$ANDROMEDA_POLICY_DIR'" "include 'andromeda-derived.conf'" "$end" >> "$conf"

	andromeda_check_memory_budget "$limit_mb" "$@"
}

# usage: andromeda_write_derived TUNE_MB CPUS [server-args...]
# Writes $PGDATA/andromeda-derived.conf with the hardware-dependent values the
# tuner does not compute. Rewritten on every start.
andromeda_write_derived() {
	local tune_mb="$1" cpus="$2"
	shift 2

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
	# writes then wait on WALWrite. A 32nd of shared_buffers, up to 64MB, and
	# never below the tuner's value.
	block_size="$(postgres -D "$PGDATA" "$@" -C block_size)"
	shared_kb=$(( $(postgres -D "$PGDATA" "$@" -C shared_buffers) * block_size / 1024 ))
	tuned_wal_kb=$(( $(postgres -D "$PGDATA" "$@" -C wal_buffers) * block_size / 1024 ))
	wal_kb=$(( shared_kb / 32 ))
	if [ "$wal_kb" -gt 65536 ]; then
		wal_kb=65536
	fi
	if [ "$wal_kb" -lt "$tuned_wal_kb" ]; then
		wal_kb="$tuned_wal_kb"
	fi

	local storage
	case "${ANDROMEDA_PG_STORAGE:-ssd}" in
		ssd)
			storage='random_page_cost = 1.1
effective_io_concurrency = 256
maintenance_io_concurrency = 256
io_combine_limit = 128kB'
			;;
		*)
			printf >&2 "error: ANDROMEDA_PG_STORAGE must be 'ssd', got '%s'\n" "$ANDROMEDA_PG_STORAGE"
			exit 1
			;;
	esac

	cat > "$PGDATA/andromeda-derived.conf" <<-EOF
		# Written by the entrypoint on every start; edits are lost. Override with ALTER SYSTEM.
		autovacuum_work_mem = ${autovacuum_mb}MB
		io_workers = ${io_workers}
		temp_file_limit = $(( data_kb * 5 / 100 ))kB
		wal_buffers = ${wal_kb}kB
		${storage}
	EOF
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

if ! _is_sourced; then
	_main "$@"
fi
