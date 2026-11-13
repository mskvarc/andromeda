#!/usr/bin/env bash
# usage: action-right-check.sh IMAGE BROKER_IMAGE
# Asserts that automatic maintenance, run as a role that owns no tenant table
# and holds MAINTAIN on every heap it vacuums, takes away only the actions
# whose own right the role lacks: it records a partial history chunk and a
# closed rowstore chunk awaiting its rebuild as would-act, logs a leftover
# index it may not drop as would-drop-artifact, runs none of their statements,
# and still runs the queued analyze of entities with the leftover in place.
# IMAGE is the TSL edition, which has a columnstore.
#
# A superuser broker creates the tenant, its history and its entities, and its
# sweep converts an aged chunk that an UPDATE then leaves partial. With that
# broker gone, a second aged chunk is filled and listed as one the sweep keeps
# in the rowstore, and an invalid idx_ca_value_number_ccnew is left on
# current_attributes. The broker is then restarted in automatic mode as a role
# that is no superuser, holds the data privileges and MAINTAIN on every table
# of public, and holds no MAINTAIN on any history chunk. That is the state a
# broker is in once database.connection moves to a new role while the tables stay
# owned by the old one.
#
# The controller runs the responsive profile: the stale entities statistics
# qualify after a breach run of three minutes and are analyzed on a later
# visit; the check waits up to fifteen minutes for it.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="action-right-check-$$"
network="$prefix-net"
database="$prefix-db"
setup="$prefix-setup"
broker="$prefix-broker"
tenant=arc
role=steward
failures=0

cleanup() {
	docker rm -f "$setup" "$broker" "$database" > /dev/null 2>&1 || :
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

# usage: logged DESCRIPTION PATTERN...
# Passes when one broker log line of the tenant matches every pattern.
logged() {
	local description="$1" lines
	shift
	lines="$(grep "$tenant" <<< "$logs" || :)"
	for pattern in "$@"; do
		lines="$(grep -e "$pattern" <<< "$lines" || :)"
	done
	if [ -n "$lines" ]; then
		pass "$description"
	else
		fail "$description"
	fi
}

# usage: start_broker NAME DATABASE_URL MODE
start_broker() {
	docker run -d --name "$1" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__CONNECTION=$2" \
		-e "ANDROMEDA__DATABASE__MAINTENANCE__MODE=$3" \
		-e ANDROMEDA__DATABASE__MAINTENANCE__PROFILE=responsive \
		-e ANDROMEDA__TEMPORAL__STORAGE__COLUMNSTORE__SWEEP_INTERVAL=2s \
		"$broker_image" > /dev/null
	base="http://$(docker port "$1" 1056/tcp)/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if docker exec "$1" andromeda health > /dev/null 2>&1; then
			return 0
		fi
		sleep 0.5
	done
	printf 'FAIL broker %s did not become healthy\n' "$1" >&2
	docker logs "$1" >&2 || :
	exit 1
}

# usage: grant_role DATABASE
# The data privileges on every table and sequence of every schema the broker
# writes, MAINTAIN on every table of public, and CREATE on public, which the
# migrator needs to confirm its ledger table exists.
grant_role() {
	sql "$1" "DO \$\$
	DECLARE
		schema_name name;
	BEGIN
		FOR schema_name IN
			SELECT nspname FROM pg_namespace
			WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema'
		LOOP
			EXECUTE format('GRANT USAGE ON SCHEMA %I TO $role', schema_name);
			EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA %I TO $role', schema_name);
			EXECUTE format('GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA %I TO $role', schema_name);
		END LOOP;
		GRANT MAINTAIN ON ALL TABLES IN SCHEMA public TO $role;
		GRANT CREATE ON SCHEMA public TO $role;
	END
	\$\$;" > /dev/null
}

# usage: create_batch FIRST LAST
create_batch() {
	local body
	body="$(seq "$1" "$2" | awk 'BEGIN { printf "[" } NR > 1 { printf "," } { printf "{\"id\":\"urn:ngsi-ld:Probe:%d\",\"type\":\"Probe\",\"temperature\":{\"type\":\"Property\",\"value\":%d}}", $1, $1 } END { printf "]" }')"
	curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
		--data "$body" "$base/entityOperations/create"
}

# usage: age_history AGE ROWS STEP
# Copies the tenant's one history row ROWS times, recorded AGE ago and STEP
# apart. History is partitioned on the write clock, so the copies land in a
# chunk whose range ended long ago.
age_history() {
	sql "$db" "CREATE TEMP TABLE aged AS SELECT * FROM attribute_history LIMIT 1;
		INSERT INTO attribute_history SELECT
			now() - interval '$1' + n * interval '$3', gen_random_uuid(), a.stream_key, a.write_ordinal, a.attribute_kind, a.value_kind,
			a.observed_at, a.created_at, a.modified_at, a.deleted_at, a.value_date_time, a.expires_at,
			CASE WHEN a.generated_instance_id IS NULL THEN NULL ELSE gen_random_uuid() END,
			a.value_type_key, a.value_datatype_key, a.unit_code_id, a.value_boolean, a.verbatim_instance_id, a.value_number, a.value_string,
			a.value_language, a.value_geometry, a.value_geometry_valid, a.value_geometry_reach, a.value_geometry_members, a.value_object, a.value_uri,
			a.object_type_keys, a.proof_seals, a.sub_attributes
		FROM aged a CROSS JOIN generate_series(1, $2) n;" > /dev/null
}

# The status of the chunk sixty days back: 1 converted, 9 converted and
# partial.
partial_status="SELECT string_agg(c.status::text, ',') FROM timescaledb_information.chunks ch JOIN _timescaledb_catalog.chunk c ON c.relid = format('%I.%I', ch.chunk_schema, ch.chunk_name)::regclass WHERE ch.hypertable_name = 'attribute_history' AND ch.range_end <= now() - interval '30 days' AND ch.range_start > now() - interval '90 days'"
# The chunks a hundred and twenty days back, which stay in the rowstore.
kept_chunks="FROM timescaledb_information.chunks WHERE hypertable_name = 'attribute_history' AND range_end <= now() - interval '90 days'"

docker network create "$network" > /dev/null
docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$image" > /dev/null
for _ in $(seq 1 120); do
	if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
		break
	fi
	sleep 0.5
done

start_broker "$setup" "postgres://andromeda:check@$database:5432/andromeda" disabled
status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
	--data '{"id":"urn:ngsi-ld:Probe:0","type":"Probe","temperature":[{"type":"Property","value":0,"observedAt":"2026-10-17T10:00:00Z"}]}' \
	"$base/temporal/entities")"
expect "create temporal entity" 201 "$status"
db="$(sql andromeda "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'")"
sql "$db" "ALTER TABLE entities SET (autovacuum_enabled = false)" > /dev/null
for first in 1 501 1001 1501; do
	expect "create entities $first to $((first + 499))" 201 "$(create_batch "$first" "$((first + 499))")"
done
# Eight instances of the one stream make the chunk pay to convert.
age_history '60 days' 8 '1 minute'
wait_for "the sweep converts the chunk sixty days back" 1 60 "$db" "$partial_status"
sql "$db" "UPDATE attribute_history SET modified_at = modified_at WHERE recorded_at <= now() - interval '30 days'" > /dev/null
expect "an UPDATE into the converted chunk leaves it partial" 9 "$(sql "$db" "$partial_status")"
docker rm -f "$setup" > /dev/null

# Four hundred thousand rows a tenth of a second apart span eleven hours, one
# chunk or two, whose indexes outgrow the responsive profile's 16 MiB floor.
# No sweep runs while they are written, and listing them as kept is what the
# sweep records for a chunk it leaves in the rowstore.
age_history '120 days' 400000 '100 milliseconds'
sql "$db" "INSERT INTO rowstore_history_chunks (range_start, row_count, stream_count, kept_at) SELECT range_start, 400000, 1, now() $kept_chunks" > /dev/null
index_bytes="$(sql "$db" "SELECT coalesce(max(pg_indexes_size(format('%I.%I', chunk_schema, chunk_name)::regclass)), 0) $kept_chunks")"
if [ "$index_bytes" -lt $((16 * 1024 * 1024)) ]; then
	fail "a kept chunk's indexes reach 16 MiB: $index_bytes bytes"
	exit 1
fi
pass "a kept chunk's indexes reach 16 MiB: $index_bytes bytes"

# Two rewrites of every entity leave its statistics stale past twice the
# analyze trigger.
sql "$db" "UPDATE entities SET modified_at = modified_at; UPDATE entities SET modified_at = modified_at" > /dev/null
# A cancelled REINDEX INDEX CONCURRENTLY leaves the index it was building
# invalid under the _ccnew name.
sql "$db" "CREATE INDEX idx_ca_value_number_ccnew ON current_attributes (value_number);
	UPDATE pg_index SET indisvalid = false WHERE indexrelid = 'public.idx_ca_value_number_ccnew'::regclass" > /dev/null

sql andromeda "CREATE ROLE $role LOGIN PASSWORD 'check'; GRANT pg_read_all_stats, pg_stat_scan_tables TO $role" > /dev/null
grant_role andromeda
grant_role "$db"
sql "$db" "DO \$\$
DECLARE
	chunk regclass;
BEGIN
	FOR chunk IN SELECT show_chunks('attribute_history') LOOP
		EXECUTE format('REVOKE MAINTAIN ON %s FROM $role', chunk);
	END LOOP;
END
\$\$;" > /dev/null
# The oldest kept chunk over the floor, the one a rebuild would work on.
chunk="$(sql "$db" "SELECT format('%I.%I', chunk_schema, chunk_name) $kept_chunks AND pg_indexes_size(format('%I.%I', chunk_schema, chunk_name)::regclass) >= 16777216 ORDER BY range_start LIMIT 1")"
expect "the role holds MAINTAIN on current_attributes and entities and none on the kept chunk" "t,t,f" \
	"$(sql "$db" "SELECT concat_ws(',', has_table_privilege('$role', 'public.current_attributes', 'MAINTAIN'), has_table_privilege('$role', 'public.entities', 'MAINTAIN'), has_table_privilege('$role', '$chunk', 'MAINTAIN'))")"
expect "the role owns neither attribute_history nor current_attributes" "false,false" \
	"$(sql "$db" "SELECT string_agg(pg_has_role('$role', relowner, 'USAGE')::text, ',' ORDER BY relname) FROM pg_class WHERE oid IN ('public.attribute_history'::regclass, 'public.current_attributes'::regclass)")"

start_broker "$broker" "postgres://$role:check@$database:5432/andromeda" automatic
key="$(sql andromeda "SELECT tenant_key FROM tenants WHERE name = '$tenant'")"

# Target 23 is entities_statistics, 35 attribute_history_columnstore and 44
# attribute_history_rowstore; snapshot key 0 is the live branch, result 1
# succeeded and decision 3 would-act.
wait_for "the queued analyze of entities runs beside the leftover index" 1 900 andromeda \
	"SELECT coalesce(string_agg(last_action_result::text, ','), 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 23"
# The two history targets qualify on breach runs of their own, which need not
# have lasted the qualifying duration yet when the analyze runs, so each waits
# past it.
wait_for "the partial chunk is recorded as would-act" 3 300 andromeda \
	"SELECT coalesce(string_agg(last_decision::text, ','), 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 35"
wait_for "the kept chunk awaiting its rebuild is recorded as would-act" 3 300 andromeda \
	"SELECT coalesce(string_agg(last_decision::text, ','), 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 44"
expect "neither history action records a result or a queued action" 0 \
	"$(sql andromeda "SELECT count(*) FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target IN (35, 44) AND (last_action_result IS NOT NULL OR is_action_pending)")"
expect "the chunk stays partial" 9 "$(sql "$db" "$partial_status")"
expect "no chunk is recorded as rebuilt" 0 "$(sql "$db" 'SELECT count(*) FROM rebuilt_history_chunks')"
expect "the leftover index is still there and invalid" f \
	"$(sql "$db" "SELECT indisvalid FROM pg_index WHERE indexrelid = 'public.idx_ca_value_number_ccnew'::regclass")"

logs="$(docker logs "$broker" 2>&1)"
logged "the missing ownership of attribute_history is logged" 'capability-unavailable' 'missing-ownership' 'public.attribute_history'
logged "the missing MAINTAIN on the kept chunk is logged" 'capability-unavailable' 'missing-maintain' "$chunk"
logged "the missing ownership of current_attributes is logged for the leftover" 'capability-unavailable' 'missing-ownership' 'idx_ca_value_number_ccnew' 'public.current_attributes'
logged "the leftover index is reported as would-drop-artifact" 'would-drop-artifact' 'idx_ca_value_number_ccnew'
logged "the analyze of entities logs acted" 'action completed' 'acted' 'entities'
if grep "$tenant" <<< "$logs" | grep -qE 'action-failed|artifact-drop-failed'; then
	fail "a statement ran without its right"
	grep "$tenant" <<< "$logs" | grep -E 'action-failed|artifact-drop-failed' >&2
else
	pass "no statement ran without its right"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
