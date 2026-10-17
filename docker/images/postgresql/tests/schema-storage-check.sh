#!/usr/bin/env bash
# usage: schema-storage-check.sh IMAGE BROKER_IMAGE
# Asserts the storage decisions the migrations and the storage-policy
# reconciler leave in a tenant database the broker creates on IMAGE: the
# declared columnstore sparse indexes, the history BRIN parameters on the
# hypertable and its chunk, the vacuum parameters of the queue and
# current-state tables and of the history tables, and the cached identity
# sequences. IMAGE is the TSL edition, whose columnstore the reconciler enables.
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

docker network create "$network" > /dev/null
docker run -d --name "$database" --network "$network" -e POSTGRES_PASSWORD=check "$image" > /dev/null
for _ in $(seq 1 120); do
	if docker exec "$database" pg_isready -q -h 127.0.0.1 2> /dev/null; then
		break
	fi
	sleep 0.5
done
docker run -d --name "$broker" --network "$network" -p 127.0.0.1::1056 \
	-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
	-e "ANDROMEDA__DATABASE__URL=postgres://postgres:check@$database:5432/postgres" \
	"$broker_image" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
		break
	fi
	sleep 0.5
done

# A temporal write creates the first chunk of attribute_history.
status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST -H "NGSILD-Tenant: $tenant" -H 'Content-Type: application/json' \
	--data '{"id":"urn:ngsi-ld:Probe:1","type":"Probe","temperature":[{"type":"Property","value":1,"observedAt":"2026-10-17T10:00:00Z"}]}' \
	"$base/temporal/entities")"
expect "create temporal entity" 201 "$status"
db="$(sql postgres "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'")"

expect "configured sparse indexes of attribute_history" \
	"bloom(generated_instance_id) bloom(recorded_at,write_id,write_ordinal) bloom(verbatim_instance_id) minmax(created_at) minmax(deleted_at) minmax(modified_at) minmax(observed_at)" \
	"$(sql "$db" "SELECT string_agg(summary, ' ' ORDER BY summary) FROM (SELECT format('%s(%s)', entry ->> 'type', (SELECT string_agg(name, ',') FROM jsonb_array_elements_text(jsonb_path_query_array(entry, '\$.column[*]')) AS name)) AS summary FROM _timescaledb_catalog.compression_settings, jsonb_path_query(index, '\$[*] ? (@.source == \"config\")') AS entry WHERE relid = 'attribute_history'::regclass) AS declared")"

expect "history chunks" 1 "$(sql "$db" "SELECT count(*) FROM show_chunks('attribute_history')")"
expect "chunk statistics recorded for chunk skipping" 0 "$(sql "$db" "SELECT count(*) FROM _timescaledb_catalog.chunk_column_stats")"

brin_options="{pages_per_range=32,autosummarize=on}"
for index in idx_attribute_history_observed_at_brin idx_attribute_history_created_at_brin idx_attribute_history_modified_at_brin idx_attribute_history_deleted_at_brin \
	idx_scope_history_observed_at_brin idx_scope_history_created_at_brin idx_scope_history_modified_at_brin idx_scope_history_deleted_at_brin; do
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

for table in entities entity_search entity_type_members entity_type_census entity_type_attribute_census attribute_census current_attributes \
	subscription_delivery_state registration_delivery_state snapshot_preparation notification_events notification_deliveries pending_side_effects temporal_append_intents; do
	expect "$table vacuum parameters" "vacuum_truncate=false autovacuum_vacuum_scale_factor=0 threshold" \
		"$(sql "$db" "SELECT string_agg(CASE WHEN option_name = 'autovacuum_vacuum_threshold' THEN 'threshold' ELSE option_name || '=' || option_value END, ' ' ORDER BY option_name <> 'vacuum_truncate', option_name) FROM pg_options_to_table((SELECT reloptions FROM pg_class WHERE oid = 'public.$table'::regclass)) WHERE option_name IN ('vacuum_truncate', 'autovacuum_vacuum_scale_factor') OR (option_name = 'autovacuum_vacuum_threshold' AND option_value::integer > 0)")"
done

for column in notification_events.event_key notification_deliveries.delivery_key attribute_names.attribute_name_key entity_type_names.entity_type_name_key temporal_append_intents.intent_key; do
	expected=32
	[ "$column" = temporal_append_intents.intent_key ] && expected=1
	expect "$column sequence cache" "$expected" \
		"$(sql "$db" "SELECT cache_size FROM pg_sequences WHERE format('%I.%I', schemaname, sequencename)::regclass = pg_get_serial_sequence('public.${column%%.*}', '${column#*.}')::regclass")"
done

# Reopening the tenant runs the reconciler against the columnstore it enabled,
# which it must find unchanged rather than drifted.
docker restart "$broker" > /dev/null
base="http://$(docker port "$broker" 1056/tcp)/ngsi-ld/v1"
for _ in $(seq 1 120); do
	if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
		break
	fi
	sleep 0.5
done
expect "retrieve temporal entity after a restart" 200 \
	"$(curl -sS -o /dev/null -w '%{http_code}' -H "NGSILD-Tenant: $tenant" "$base/temporal/entities/urn:ngsi-ld:Probe:1")"
if docker logs "$broker" 2>&1 | grep -qE 'ERROR|not the settings the broker enables'; then
	fail "broker log carries an error"
	docker logs "$broker" 2>&1 | grep -E 'ERROR|not the settings the broker enables' >&2
else
	pass "broker log carries no error"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
