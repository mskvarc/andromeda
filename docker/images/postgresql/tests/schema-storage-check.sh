#!/usr/bin/env bash
# usage: schema-storage-check.sh IMAGE BROKER_IMAGE
# Asserts the storage decisions the migrations and the storage-policy
# reconciler leave in a tenant database the broker creates on IMAGE: the
# declared columnstore sparse indexes, the history BRIN parameters on the
# hypertable and its chunk, the vacuum parameters of the queue and
# current-state tables and of the history tables, the storage parameters of
# the temporal Evolution table, the cached identity
# sequences, the absence of any TimescaleDB conversion job, and the session
# defaults the broker installs on the tenant and the platform database. IMAGE
# is the TSL edition, whose columnstore the reconciler enables. On a server
# running TimescaleDB's background workers, it then asserts that the tenant
# template holds no session across a broker start and that every tenant
# created after one is copied from it.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="schema-storage-check-$$"
network="$prefix-net"
database="$prefix-db"
broker="$prefix-broker"
tenant=sto
failures=0

cleanup() {
	docker rm -f -v "$broker" "$database" > /dev/null 2>&1 || :
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

# usage: start_database [docker-run-args...]
start_database() {
	docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$@" "$image" > /dev/null
	for _ in $(seq 1 120); do
		if [ "$(docker inspect -f '{{.State.Health.Status}}' "$database")" = healthy ]; then
			return 0
		fi
		sleep 0.5
	done
}

# usage: wait_broker; sets base to the broker's API once it is healthy.
wait_broker() {
	base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if docker exec "$broker" andromeda health > /dev/null 2>&1; then
			return 0
		fi
		sleep 0.5
	done
}

start_broker() {
	docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__CONNECTION=postgres://andromeda:check@$database:5432/andromeda" \
		"$broker_image" > /dev/null
	wait_broker
}

docker network create "$network" > /dev/null
start_database
start_broker

# A temporal write creates the first chunk of attribute_history.
status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
	--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":[{"type":"Property","value":1,"observedAt":"2026-10-17T10:00:00Z"}]}' \
	"$base/temporal/entities")"
expect "create temporal entity" 201 "$status"
db="$(sql andromeda "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'")"

expect "configured sparse indexes of attribute_history" \
	"bloom(generated_instance_id) bloom(verbatim_instance_id) minmax(created_at) minmax(deleted_at) minmax(modified_at) minmax(observed_at)" \
	"$(sql "$db" "SELECT string_agg(summary, ' ' ORDER BY summary) FROM (SELECT format('%s(%s)', entry ->> 'type', (SELECT string_agg(name, ',') FROM jsonb_array_elements_text(jsonb_path_query_array(entry, '\$.column[*]')) AS name)) AS summary FROM _timescaledb_catalog.compression_settings, jsonb_path_query(index, '\$[*] ? (@.source == \"config\")') AS entry WHERE relid = 'attribute_history'::regclass) AS declared")"

expect "history chunks" 1 "$(sql "$db" "SELECT count(*) FROM show_chunks('attribute_history')")"
expect "columns registered for chunk skipping" "created_at modified_at observed_at" \
	"$(sql "$db" "SELECT string_agg(column_name::text, ' ' ORDER BY column_name) FROM _timescaledb_catalog.chunk_column_stats WHERE chunk_id IS NULL")"
expect "chunk skipping on in every session" on "$(sql "$db" 'SHOW timescaledb.enable_chunk_skipping')"
expect "conversions merging chunks" 0 "$(sql "$db" "SELECT count(*) FROM _timescaledb_catalog.dimension WHERE compress_interval_length IS NOT NULL")"
expect "unique indexes on attribute_history" 0 "$(sql "$db" "SELECT count(*) FROM pg_index WHERE indrelid = 'attribute_history'::regclass AND indisunique")"

brin_options="{pages_per_range=32,autosummarize=on}"
for index in idx_attribute_history_observed_at_brin idx_attribute_history_created_at_brin idx_attribute_history_modified_at_brin idx_attribute_history_deleted_at_brin; do
	expect "$index" "$brin_options" "$(sql "$db" "SELECT reloptions FROM pg_class WHERE oid = 'public.$index'::regclass")"
done
expect "BRIN indexes of the attribute_history chunk" "4 $brin_options" \
	"$(sql "$db" "SELECT count(*) || ' ' || string_agg(DISTINCT c.reloptions::text, ' ') FROM show_chunks('attribute_history') AS chunk JOIN pg_index i ON i.indrelid = chunk JOIN pg_class c ON c.oid = i.indexrelid JOIN pg_am am ON am.oid = c.relam WHERE am.amname = 'brin'")"

for table in attribute_history scope_history; do
	expect "$table insert scale factor" 0.05 \
		"$(sql "$db" "SELECT option_value FROM pg_options_to_table((SELECT reloptions FROM pg_class WHERE oid = 'public.$table'::regclass)) WHERE option_name = 'autovacuum_vacuum_insert_scale_factor'")"
done
expect "attribute_history chunk insert scale factor" 0.05 \
	"$(sql "$db" "SELECT option_value FROM show_chunks('attribute_history') AS chunk, pg_options_to_table((SELECT reloptions FROM pg_class WHERE oid = chunk)) WHERE option_name = 'autovacuum_vacuum_insert_scale_factor'")"

for table in entities entity_search entity_type_members entity_type_census entity_type_attribute_census attribute_census stream_type_census current_attributes \
	subscription_delivery_state registration_delivery_state snapshot_preparation notification_events notification_deliveries pending_side_effects temporal_append_intents; do
	expect "$table vacuum parameters" "vacuum_truncate=false autovacuum_vacuum_scale_factor=0 threshold" \
		"$(sql "$db" "SELECT string_agg(CASE WHEN option_name = 'autovacuum_vacuum_threshold' THEN 'threshold' ELSE option_name || '=' || option_value END, ' ' ORDER BY option_name <> 'vacuum_truncate', option_name) FROM pg_options_to_table((SELECT reloptions FROM pg_class WHERE oid = 'public.$table'::regclass)) WHERE option_name IN ('vacuum_truncate', 'autovacuum_vacuum_scale_factor') OR (option_name = 'autovacuum_vacuum_threshold' AND option_value::integer > 0)")"
done

expect "entities storage parameters" \
	"{vacuum_truncate=false,fillfactor=70,autovacuum_vacuum_scale_factor=0,autovacuum_vacuum_threshold=500,autovacuum_analyze_scale_factor=0.02,autovacuum_analyze_threshold=500,autovacuum_vacuum_cost_delay=0}" \
	"$(sql "$db" "SELECT reloptions FROM pg_class WHERE oid = 'public.entities'::regclass")"

for column in notification_events.event_key notification_deliveries.delivery_key attribute_names.attribute_name_key entity_type_names.entity_type_name_key temporal_append_intents.intent_key; do
	expected=32
	[ "$column" = temporal_append_intents.intent_key ] && expected=1
	expect "$column sequence cache" "$expected" \
		"$(sql "$db" "SELECT cache_size FROM pg_sequences WHERE format('%I.%I', schemaname, sequencename)::regclass = pg_get_serial_sequence('public.${column%%.*}', '${column#*.}')::regclass")"
done

expect "TimescaleDB jobs on attribute_history" 0 \
	"$(sql "$db" "SELECT count(*) FROM timescaledb_information.jobs WHERE hypertable_name = 'attribute_history'")"

# The default request timeout of 30 s plus the 5 s margin.
for database_name in "$db" andromeda; do
	expect "$database_name session defaults" "client_connection_check_interval=1000ms statement_timeout=35000ms synchronous_commit=on timescaledb.enable_chunk_skipping=on" \
		"$(sql andromeda "SELECT string_agg(setting, ' ' ORDER BY setting) FROM pg_db_role_setting, unnest(setconfig) AS setting WHERE setrole = 0 AND setdatabase = (SELECT oid FROM pg_database WHERE datname = '$database_name')")"
done
expect "a tenant session runs under the installed defaults" "on 35s" "$(sql "$db" "SELECT current_setting('synchronous_commit') || ' ' || current_setting('statement_timeout')")"

# Reopening the tenant runs the reconciler against the columnstore it enabled,
# which it must find unchanged rather than drifted.
docker restart "$broker" > /dev/null
wait_broker
expect "retrieve temporal entity after a restart" 200 \
	"$(curl -sS -o /dev/null -w '%{http_code}' -H "NGSILD-Tenant: $tenant" "$base/temporal/entities/urn:ngsi-ld:Probe:1")"
if docker logs "$broker" 2>&1 | grep -qE 'ERROR|not the settings the broker enables'; then
	fail "broker log carries an error"
	docker logs "$broker" 2>&1 | grep -E 'ERROR|not the settings the broker enables' >&2
else
	pass "broker log carries no error"
fi

# With TimescaleDB's background workers on, the launcher keeps a scheduler on
# every database that holds the extension and admits connections. The tenant
# template admits none once locked, so the broker start that brings it forward
# leaves no session on it, and no copy waits on one.
docker rm -f -v "$broker" "$database" > /dev/null
start_database -e TS_TUNE_MAX_BG_WORKERS=tune
start_broker
for name in wa wb; do
	tenant="$name"
	expect "create entity in $name with background workers on" 201 \
		"$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
			--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":{"type":"Property","value":1}}' "$base/entities")"
done
docker restart "$broker" > /dev/null
wait_broker
for name in wc wd; do
	tenant="$name"
	expect "create entity in $name with background workers on" 201 \
		"$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
			--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":{"type":"Property","value":1}}' "$base/entities")"
done
expect "sessions on the tenant template with background workers on" 0 \
	"$(sql andromeda "SELECT count(*) FROM pg_stat_activity a JOIN pg_database d ON d.datname = a.datname WHERE d.datistemplate AND d.datname NOT IN ('template0', 'template1')")"
# A copy keeps the template's ledger instants, which databases migrated one
# after the other never share.
first_migrations=''
for name in wa wb wc wd; do
	tenant_db="$(sql andromeda "SELECT database_name FROM tenants WHERE name = '$name'")"
	first_migrations="$first_migrations$(sql "$tenant_db" 'SELECT min(installed_on) FROM _andromeda_tenant_migrations')
"
done
expect "first migration instants of the four tenants with background workers on" 1 "$(printf '%s' "$first_migrations" | sort -u | grep -c .)"
if docker logs "$broker" 2>&1 | grep -qE 'ERROR|tenant_template_not_copied|tenant_template_unavailable'; then
	fail "broker log with background workers on carries an error or a tenant created without the template"
	docker logs "$broker" 2>&1 | grep -E 'ERROR|tenant_template_not_copied|tenant_template_unavailable' >&2
else
	pass "broker log with background workers on carries no error and every tenant was copied"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
