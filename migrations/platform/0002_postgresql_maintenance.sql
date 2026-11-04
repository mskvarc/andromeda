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
-- A density-measured GIN is judged against the size its covered rows take at
-- its keys per row and its bytes per key, and its degradation is its size
-- above that. `keys_per_row` is its keys per covered row, in thousandths of a
-- key, sampled from its table once for the rows of each analyze, so values
-- that yield more keys raise the size the GIN is judged against instead of
-- reading as degradation. `keys_analyzed_at` is the analyze the last sample
-- was taken for, set even when the sample read no covered row, so a GIN few
-- rows hold is sampled once per analyze and not on every visit. `key_bytes`
-- is its bytes per key, in thousandths of a byte, measured after each
-- successful rebuild, and on its first observation for a GIN of a shape with
-- no default; until it is measured, a trigram GIN is judged at its shape's
-- default. All three stay NULL for every other target.
--
-- `analyzed_rows` is the row count a target is judged by, as its table's
-- latest analyze left it: the live rows of a heap, the rows holding a value in
-- the column a density-measured GIN covers. `rows_analyzed_at` is the instant
-- of that analyze. A vacuum extrapolates a table's row count from the pages it
-- scanned, which under writes are the pages the writes touched, so the stored
-- count is replaced only by a reading taken after an analyze newer than both
-- the stored instant and the table's latest vacuum. Both stay NULL for every
-- other target, and until a visit reads a target after an analyze.
--
-- A heap's planner statistics are a target of their own (targets 22 to 30
-- and 38), refreshed by `ANALYZE`: an analyze row records no sample evidence
-- and neither reading pair, only its decision, its pending action and its
-- cooldown. So does the columnstore of a history hypertable (target 35),
-- whose partially converted chunks are recompressed one at a time, and its
-- kept rowstore chunks (target 44), whose indexes are rebuilt one chunk at a
-- time.
--
-- A target row belongs to one context branch of its tenant: `snapshot_key`
-- is the Snapshot whose published branch schema holds the relation, 0 for the
-- live branch, which no Snapshot key is. The grain is the primary key, so its
-- branch member is never NULL and compares with plain `=`. A Snapshot key
-- lives in the tenant database, so no foreign key can hold it; a visit
-- deletes the rows of a branch whose schema no longer exists.
--
-- The values behind a target's latest decision (`last_relation_bytes`,
-- `last_reclaimable_bytes`, `last_degraded_percent`,
-- `last_threshold_percent`, `last_decision`) let an operator see why a pass
-- acted, queued or deferred work without reconstructing it from logs. Each
-- stays NULL until the target's first sample.
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
--   target: 1 pk_ca_grain, 2 idx_ca_value_number,
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
--     32 uq_notification_deliveries_event_subscription,
--     33 idx_notification_deliveries_subscription,
--     34 pk_notification_deliveries, 35 attribute_history_columnstore,
--     36 idx_entities_id_trgm, 37 temporal_attribute_streams,
--     38 temporal_attribute_streams_statistics,
--     39 pk_temporal_attribute_streams,
--     40 uq_temporal_attribute_streams_grain,
--     41 idx_temporal_attribute_streams_attribute_name_key_entity_key,
--     42 idx_temporal_attribute_streams_value_extent,
--     43 idx_temporal_attribute_streams_attribute_name_key_shape_mask,
--     44 attribute_history_rowstore,
--     45 idx_notification_deliveries_subscription_claim
--   last_action_result: 1 succeeded, 2 failed, 3 ineffective
--   last_decision: 1 act, 2 queued, 3 would-act, 4 relation-too-small,
--     5 reclaim-too-small, 6 below-threshold, 7 consecutive-samples,
--     8 cooldown, 9 active-backend-ratio, 10 long-transaction,
--     11 active-maintenance, 12 waiting-relation-lock, 13 paired-queued,
--     14 horizon-held
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
    snapshot_key bigint not null,
    last_sampled_at timestamptz,
    cooldown_until timestamptz,
    last_action_at timestamptz,
    rows_analyzed_at timestamptz,
    keys_analyzed_at timestamptz,
    last_before_bytes bigint,
    last_after_bytes bigint,
    last_dead_tuples_before bigint,
    last_dead_tuples_after bigint,
    keys_per_row bigint,
    key_bytes bigint,
    analyzed_rows bigint,
    last_relation_bytes bigint,
    last_reclaimable_bytes bigint,
    first_breach_at timestamptz,
    breach_samples integer not null default 0,
    consecutive_failures integer not null default 0,
    target smallint not null,
    last_action_result smallint,
    last_degraded_percent smallint,
    last_threshold_percent smallint,
    last_decision smallint,
    is_action_pending boolean not null default false,
    constraint pk_postgresql_maintenance_targets primary key (tenant_key, snapshot_key, target),
    constraint fk_postgresql_maintenance_targets_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_postgresql_maintenance_targets_snapshot_key_non_negative check (snapshot_key >= 0),
    constraint chk_postgresql_maintenance_targets_target check (target between 1 and 45),
    constraint chk_postgresql_maintenance_targets_last_action_result check (last_action_result is null or last_action_result between 1 and 3),
    constraint chk_postgresql_maintenance_targets_breach_run check (breach_samples >= 0 and (first_breach_at is null) = (breach_samples = 0)),
    constraint chk_postgresql_maintenance_targets_failures_non_negative check (consecutive_failures >= 0),
    constraint chk_postgresql_maintenance_targets_sizes_non_negative check ((last_before_bytes is null or last_before_bytes >= 0) and (last_after_bytes is null or last_after_bytes >= 0)),
    constraint chk_postgresql_maintenance_targets_dead_tuples_non_negative check ((last_dead_tuples_before is null or last_dead_tuples_before >= 0) and (last_dead_tuples_after is null or last_dead_tuples_after >= 0)),
    constraint chk_postgresql_maintenance_targets_keys_per_row_positive check (keys_per_row is null or (keys_per_row > 0 and keys_analyzed_at is not null)),
    constraint chk_postgresql_maintenance_targets_key_bytes_positive check (key_bytes is null or key_bytes > 0),
    constraint chk_postgresql_maintenance_targets_analyzed_rows check ((analyzed_rows is null) = (rows_analyzed_at is null) and (analyzed_rows is null or analyzed_rows >= 0)),
    constraint chk_postgresql_maintenance_targets_sample_non_negative check ((last_relation_bytes is null or last_relation_bytes >= 0) and (last_reclaimable_bytes is null or last_reclaimable_bytes >= 0)),
    constraint chk_postgresql_maintenance_targets_percentages check ((last_degraded_percent is null or last_degraded_percent between 0 and 100) and (last_threshold_percent is null or last_threshold_percent between 0 and 100)),
    constraint chk_postgresql_maintenance_targets_last_decision check (last_decision is null or last_decision between 1 and 14)
);

create table postgresql_maintenance_fleet (
    singleton boolean not null default true,
    autovacuum_saturated_passes integer not null default 0,
    constraint pk_postgresql_maintenance_fleet primary key (singleton),
    constraint chk_postgresql_maintenance_fleet_singleton check (singleton),
    constraint chk_postgresql_maintenance_fleet_saturated_passes_non_negative check (autovacuum_saturated_passes >= 0)
);
