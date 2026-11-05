#!/usr/bin/env bash
# usage: maintain-privilege-check.sh IMAGE BROKER_IMAGE
# Asserts that automatic maintenance observes a branch whose heaps the broker
# role lacks MAINTAIN on, instead of skipping the tenant: the visit records
# its catalog samples, reports what it would do, and runs no statement there.
#
# A superuser broker creates the tenant and its entities, with autovacuum off
# on entities and attribute_census so their dead tuples and modifications
# pile up. The broker is then restarted in automatic mode as a role that is
# no superuser, holds the data privileges and MAINTAIN on every table, and
# lacks MAINTAIN on the tenant's entities only. That is the state a broker is
# in once database.url moves to a new role while the tables stay owned by the
# old one.
set -Eeuo pipefail

image="$1"
broker_image="$2"
prefix="maintain-privilege-check-$$"
network="$prefix-net"
database="$prefix-db"
setup="$prefix-setup"
broker="$prefix-broker"
tenant=mpc
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

# usage: start_broker NAME DATABASE_URL MODE
start_broker() {
	docker run -d --name "$1" --network "$network" -p 127.0.0.1::1056 \
		-e ANDROMEDA__SERVER__HOST=0.0.0.0 \
		-e "ANDROMEDA__DATABASE__URL=$2" \
		-e "ANDROMEDA__DATABASE__MAINTENANCE__MODE=$3" \
		-e ANDROMEDA__DATABASE__MAINTENANCE__PROFILE=responsive \
		"$broker_image" > /dev/null
	base="http://$(docker port "$1" 1056/tcp)/ngsi-ld/v1"
	for _ in $(seq 1 120); do
		if curl -fsS -o /dev/null "$base/health" 2> /dev/null; then
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
	--data '{"id":"urn:ngsi-ld:Probe:0","type":"Probe","temperature":{"type":"Property","value":0}}' \
	"$base/entities")"
expect "create the tenant's first entity" 201 "$status"
db="$(sql andromeda "SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_${tenant}%'")"
sql "$db" "ALTER TABLE entities SET (autovacuum_enabled = false); ALTER TABLE attribute_census SET (autovacuum_enabled = false); ALTER TABLE current_attributes SET (autovacuum_enabled = false)" > /dev/null
for first in 1 501 1001 1501; do
	expect "create entities $first to $((first + 499))" 201 "$(create_batch "$first" "$((first + 499))")"
done
docker rm -f "$setup" > /dev/null

# Two rewrites of every entity leave twice its row count dead and modified;
# the census rows cancel out, so the counts the broker serves stay right.
sql "$db" "UPDATE entities SET modified_at = modified_at; UPDATE entities SET modified_at = modified_at;
	INSERT INTO attribute_census (attribute_name_key, instance_delta)
	SELECT a.attribute_name_key, d.delta
	FROM (SELECT attribute_name_key FROM attribute_census LIMIT 1) a
	CROSS JOIN generate_series(1, 2000) n
	CROSS JOIN (VALUES (1::bigint), (-1::bigint)) d(delta);" > /dev/null
# A heap's dead share is judged on the rows its last analyze counted, so
# current_attributes, kept from autovacuum, is analyzed for the visit to have
# a heap to sample; entities stays unanalyzed for its stale statistics.
sql "$db" "ANALYZE current_attributes" > /dev/null

sql andromeda "CREATE ROLE $role LOGIN PASSWORD 'check'; GRANT pg_read_all_stats, pg_stat_scan_tables TO $role" > /dev/null
grant_role andromeda
grant_role "$db"
sql "$db" "REVOKE MAINTAIN ON entities FROM $role" > /dev/null
expect "the role lacks MAINTAIN on the tenant's entities only" "f,t" \
	"$(sql "$db" "SELECT concat_ws(',', has_table_privilege('$role', 'public.entities', 'MAINTAIN'), has_table_privilege('$role', 'public.current_attributes', 'MAINTAIN'))")"

start_broker "$broker" "postgres://$role:check@$database:5432/andromeda" automatic
key="$(sql andromeda "SELECT tenant_key FROM tenants WHERE name = '$tenant'")"

# Result 16 is would-act; 6 (missing-maintain) and 12 (capability-unavailable)
# are what a visit that stopped at the privilege check records.
wait_for "the visit records its own outcome" 16 300 andromeda \
	"SELECT coalesce(last_result::text, 'none') FROM postgresql_maintenance_tenants WHERE tenant_key = $key"
# Target 11 is current_attributes, 23 entities_statistics; decision 3 is
# would-act.
expect "the current_attributes heap is sampled" 1 \
	"$(sql andromeda "SELECT count(*) FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 11 AND last_sampled_at IS NOT NULL")"
expect "the stale entities statistics are reported as would-act" 3 \
	"$(sql andromeda "SELECT coalesce(last_decision::text, 'none') FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND target = 23")"
expect "no live target records an action result or a queued action" 0 \
	"$(sql andromeda "SELECT count(*) FROM postgresql_maintenance_targets WHERE tenant_key = $key AND snapshot_key = 0 AND (last_action_result IS NOT NULL OR is_action_pending)")"
expect "the entities statistics were never analyzed" 0 \
	"$(sql "$db" "SELECT analyze_count + autoanalyze_count FROM pg_stat_user_tables WHERE relid = 'public.entities'::regclass")"

logs="$(docker logs "$broker" 2>&1)"
missing="$(grep 'missing-maintain' <<< "$logs" | grep "$tenant" || :)"
if [ -n "$missing" ] && grep -q 'capability-unavailable' <<< "$missing" && grep -q 'Live' <<< "$missing"; then
	pass "a capability-unavailable warning names the live branch"
else
	fail "no capability-unavailable warning names the live branch"
fi
if grep 'autovacuum-overdue' <<< "$logs" | grep "$tenant" | grep -q 'entities'; then
	pass "autovacuum-overdue is reported for entities"
else
	fail "autovacuum-overdue is not reported for entities"
fi
if grep 'statistics-stale' <<< "$logs" | grep "$tenant" | grep -q 'attribute_census'; then
	pass "statistics-stale is reported for attribute_census"
else
	fail "statistics-stale is not reported for attribute_census"
fi
if grep -qE 'ERROR' <<< "$logs"; then
	fail "broker log carries an error"
	grep -E 'ERROR' <<< "$logs" >&2
else
	pass "broker log carries no error"
fi

if [ "$failures" -gt 0 ]; then
	printf '%d check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'all checks passed\n'
