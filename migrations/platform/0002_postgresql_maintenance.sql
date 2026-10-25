-- ============================================================================
-- 0002: PostgreSQL maintenance state
-- ============================================================================
-- What the opt-in maintenance controller knows about each tenant and each
-- relation it may act on. Every broker replica runs the controller, and the
-- platform database is where the replicas coordinate: a tenant row keeps what
-- tenant selection orders by across restarts, including tenants no replica
-- currently holds in memory, and a target row keeps the breach evidence that
-- stops one noisy sample from triggering an action.
--
-- `last_tuple_changes` and `last_stats_reset` are the tenant database's
-- `pg_stat_database` counters (`tup_inserted + tup_updated + tup_deleted`,
-- `stats_reset`) as observed when its last visit started. The rows a database
-- changed since then are its churn; a changed `stats_reset` or a lower counter
-- means the statistics were reset, which erases the changes in between and
-- makes the tenant due on its own. Both stay NULL until a visit observes the
-- database.
--
-- A breach run is the unbroken sequence of samples meeting every threshold:
-- `first_breach_at` is when its first sample was taken and `breach_samples`
-- how many it holds. Work qualifies once a run spans the profile's qualifying
-- duration over at least two samples; a sample below threshold and a
-- completed action close the run.
--
-- A rebuild's effect is recorded as physical bytes (`last_before_bytes`,
-- `last_after_bytes`) and a vacuum's as dead tuples
-- (`last_dead_tuples_before`, `last_dead_tuples_after`): a vacuum without
-- truncation never shrinks its heap's file. The pair that does not apply to a
-- target stays NULL.
--
-- `cooldown_until` is set by every action: the profile's success cooldown
-- after an effective action, its shorter ineffective cooldown after a vacuum
-- that left the dead share at or above its threshold or a density-measured GIN
-- rebuild that reclaimed less than the profile's reclaim floor, and a backoff
-- doubling with `consecutive_failures` (capped at 24 hours) after a failure.
--
-- `baseline_density` is a density-measured GIN's bytes per covered row, in
-- thousandths of a byte, recorded on its first observation and again after
-- each successful rebuild. It stays NULL for every other target, and for a GIN
-- whose covered rows the statistics do not yet describe.
--
-- A heap's planner statistics are a target of their own (targets 22 to 30),
-- refreshed by `ANALYZE`: an analyze row records no sample evidence and
-- neither reading pair, only its decision, its pending action and its
-- cooldown. So does the columnstore of a history hypertable (target 35),
-- whose partially converted chunks are recompressed one at a time.
--
-- A target row belongs to one context branch of its tenant: `snapshot_key`
-- is the Snapshot whose published branch schema holds the relation, NULL for
-- the live branch. The grain therefore contains a nullable column and the
-- table has no primary key. A Snapshot key lives in the tenant database, so
-- no foreign key can hold it; a visit deletes the rows of a branch whose
-- schema no longer exists.
--
-- `postgresql_maintenance_fleet` holds the one row of state that belongs to
-- the cluster rather than to a tenant. `autovacuum_saturated_passes` counts
-- the consecutive passes that found every autovacuum worker slot busy; a pass
-- that finds one free resets it to zero. The row is written by the first pass.
--
-- Codes:
--
--   last_result: 1 started, 2 unavailable, 3 ready, 4 missing-pgstattuple,
--     5 missing-pgstattuple-execute, 6 missing-maintain, 7 disabled,
--     8 global-lock-unavailable, 9 no-tenant, 10 tenant-unavailable,
--     11 schema-behind, 12 capability-unavailable, 13 idle, 14 probed,
--     15 queued, 16 would-act, 17 deferred, 18 acted, 19 action-failed,
--     20 cancelled
--   target: 1 uq_current_attributes_grain, 2 idx_ca_value_number,
--     3 idx_ca_value_string, 4 idx_ca_value_string_trgm,
--     5 idx_ca_value_boolean, 6 idx_ca_value_date_time, 7 idx_ca_value_uri,
--     8 idx_ca_value_geometry, 9 idx_ca_entity_geometry,
--     10 idx_ca_sub_eq_keys, 11 current_attributes, 12 entities,
--     13 subscription_delivery_state, 14 idx_ca_value_object,
--     15 idx_ca_value_long_text_trgm, 16 notification_events,
--     17 notification_deliveries, 18 pending_side_effects,
--     19 temporal_append_intents, 20 snapshot_notification_outbox,
--     21 entity_search, 22 current_attributes_statistics,
--     23 entities_statistics, 24 subscription_delivery_state_statistics,
--     25 notification_events_statistics,
--     26 notification_deliveries_statistics,
--     27 pending_side_effects_statistics,
--     28 temporal_append_intents_statistics,
--     29 snapshot_notification_outbox_statistics,
--     30 entity_search_statistics, 31 idx_notification_deliveries_claim,
--     32 idx_notification_deliveries_event,
--     33 idx_notification_deliveries_subscription,
--     34 pk_notification_deliveries, 35 attribute_history_columnstore
--   last_action_result: 1 succeeded, 2 failed, 3 ineffective
-- ============================================================================
create table postgresql_maintenance_tenants (
    tenant_key bigint not null,
    last_inspected_at timestamptz,
    consecutive_failures integer not null default 0,
    last_result smallint,
    last_tuple_changes bigint,
    last_stats_reset timestamptz,
    constraint pk_postgresql_maintenance_tenants primary key (tenant_key),
    constraint fk_postgresql_maintenance_tenants_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_postgresql_maintenance_tenants_failures_non_negative check (consecutive_failures >= 0),
    constraint chk_postgresql_maintenance_tenants_tuple_changes_non_negative check (last_tuple_changes is null or last_tuple_changes >= 0),
    constraint chk_postgresql_maintenance_tenants_last_result check (last_result is null or last_result between 1 and 20)
);

create table postgresql_maintenance_targets (
    tenant_key bigint not null,
    snapshot_key bigint,
    last_sampled_at timestamptz,
    cooldown_until timestamptz,
    last_action_at timestamptz,
    last_before_bytes bigint,
    last_after_bytes bigint,
    last_dead_tuples_before bigint,
    last_dead_tuples_after bigint,
    baseline_density bigint,
    first_breach_at timestamptz,
    breach_samples integer not null default 0,
    consecutive_failures integer not null default 0,
    target smallint not null,
    last_action_result smallint,
    is_action_pending boolean not null default false,
    constraint uq_postgresql_maintenance_targets_grain unique nulls not distinct (tenant_key, snapshot_key, target),
    constraint fk_postgresql_maintenance_targets_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_postgresql_maintenance_targets_snapshot_key_positive check (snapshot_key is null or snapshot_key > 0),
    constraint chk_postgresql_maintenance_targets_target check (target between 1 and 35),
    constraint chk_postgresql_maintenance_targets_last_action_result check (last_action_result is null or last_action_result between 1 and 3),
    constraint chk_postgresql_maintenance_targets_breach_run check (breach_samples >= 0 and (first_breach_at is null) = (breach_samples = 0)),
    constraint chk_postgresql_maintenance_targets_failures_non_negative check (consecutive_failures >= 0),
    constraint chk_postgresql_maintenance_targets_sizes_non_negative check ((last_before_bytes is null or last_before_bytes >= 0) and (last_after_bytes is null or last_after_bytes >= 0)),
    constraint chk_postgresql_maintenance_targets_dead_tuples_non_negative check ((last_dead_tuples_before is null or last_dead_tuples_before >= 0) and (last_dead_tuples_after is null or last_dead_tuples_after >= 0)),
    constraint chk_postgresql_maintenance_targets_baseline_density_positive check (baseline_density is null or baseline_density > 0)
);

create table postgresql_maintenance_fleet (
    singleton boolean not null default true,
    autovacuum_saturated_passes integer not null default 0,
    constraint pk_postgresql_maintenance_fleet primary key (singleton),
    constraint chk_postgresql_maintenance_fleet_singleton check (singleton),
    constraint chk_postgresql_maintenance_fleet_saturated_passes_non_negative check (autovacuum_saturated_passes >= 0)
);
