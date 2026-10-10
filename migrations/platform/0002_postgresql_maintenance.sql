-- ============================================================================
-- 0002: PostgreSQL maintenance state
-- ============================================================================
-- What the opt-in maintenance controller knows about each tenant and each
-- relation it may act on. Every broker replica runs the controller, and the
-- platform database is where the replicas coordinate: `last_inspected_at`
-- keeps tenant selection fair across restarts, including tenants no replica
-- currently holds in memory, and a target row keeps the consecutive-sample
-- evidence that stops one noisy sample from triggering an action.
--
-- A rebuild's effect is recorded as physical bytes (`last_before_bytes`,
-- `last_after_bytes`) and a vacuum's as dead tuples
-- (`last_dead_tuples_before`, `last_dead_tuples_after`): a vacuum without
-- truncation never shrinks its heap's file. The pair that does not apply to a
-- target stays NULL.
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
--     13 subscription_delivery_state
--   last_action_result: 1 succeeded, 2 failed
-- ============================================================================
create table postgresql_maintenance_tenants (
    tenant_key bigint not null,
    last_inspected_at timestamptz,
    consecutive_failures integer not null default 0,
    last_result smallint,
    constraint pk_postgresql_maintenance_tenants primary key (tenant_key),
    constraint fk_postgresql_maintenance_tenants_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_postgresql_maintenance_tenants_failures_non_negative check (consecutive_failures >= 0),
    constraint chk_postgresql_maintenance_tenants_last_result check (last_result is null or last_result between 1 and 20)
);

create table postgresql_maintenance_targets (
    tenant_key bigint not null,
    last_sampled_at timestamptz,
    cooldown_until timestamptz,
    last_action_at timestamptz,
    last_before_bytes bigint,
    last_after_bytes bigint,
    last_dead_tuples_before bigint,
    last_dead_tuples_after bigint,
    consecutive_breaches integer not null default 0,
    consecutive_failures integer not null default 0,
    target smallint not null,
    last_action_result smallint,
    is_action_pending boolean not null default false,
    constraint pk_postgresql_maintenance_targets primary key (tenant_key, target),
    constraint fk_postgresql_maintenance_targets_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_postgresql_maintenance_targets_target check (target between 1 and 13),
    constraint chk_postgresql_maintenance_targets_last_action_result check (last_action_result is null or last_action_result between 1 and 2),
    constraint chk_postgresql_maintenance_targets_breaches_non_negative check (consecutive_breaches >= 0),
    constraint chk_postgresql_maintenance_targets_failures_non_negative check (consecutive_failures >= 0),
    constraint chk_postgresql_maintenance_targets_sizes_non_negative check ((last_before_bytes is null or last_before_bytes >= 0) and (last_after_bytes is null or last_after_bytes >= 0)),
    constraint chk_postgresql_maintenance_targets_dead_tuples_non_negative check ((last_dead_tuples_before is null or last_dead_tuples_before >= 0) and (last_dead_tuples_after is null or last_dead_tuples_after >= 0))
);

-- Tenant selection: never-inspected tenants first, then the least recently
-- inspected.
create index idx_postgresql_maintenance_tenants_last_inspected_at on postgresql_maintenance_tenants (last_inspected_at nulls first, tenant_key);

