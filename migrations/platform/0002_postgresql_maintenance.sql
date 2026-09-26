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
-- Codes:
--
--   last_result: 1 started, 2 unavailable, 3 ready, 4 missing-pgstattuple,
--     5 missing-pgstattuple-execute, 6 missing-maintain, 7 disabled,
--     8 global-lock-unavailable, 9 no-tenant, 10 tenant-unavailable,
--     11 schema-behind, 12 capability-unavailable, 13 idle, 14 probed,
--     15 queued, 16 would-act, 17 deferred, 18 acted, 19 action-failed,
--     20 cancelled
--   target: 1 uq_current_attribute_values_grain, 2 idx_cav_value_number,
--     3 idx_cav_value_string, 4 idx_cav_value_string_trgm,
--     5 idx_cav_value_boolean, 6 idx_cav_value_date_time, 7 idx_cav_value_uri,
--     8 idx_cav_value_geometry, 9 idx_cav_entity_geometry,
--     10 current_attribute_values, 11 entity_documents,
--     12 subscription_delivery_state
--   last_action_result: 1 succeeded, 2 failed
-- ============================================================================

CREATE TABLE postgresql_maintenance_tenants (
    tenant_key BIGINT NOT NULL,
    last_inspected_at TIMESTAMPTZ,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_result SMALLINT,
    CONSTRAINT pk_postgresql_maintenance_tenants PRIMARY KEY (tenant_key),
    CONSTRAINT fk_postgresql_maintenance_tenants_tenant FOREIGN KEY (tenant_key)
        REFERENCES tenants(tenant_key) ON DELETE CASCADE,
    CONSTRAINT chk_postgresql_maintenance_tenants_failures_non_negative
        CHECK (consecutive_failures >= 0),
    CONSTRAINT chk_postgresql_maintenance_tenants_last_result
        CHECK (last_result IS NULL OR last_result BETWEEN 1 AND 20)
);

CREATE TABLE postgresql_maintenance_targets (
    tenant_key BIGINT NOT NULL,
    last_sampled_at TIMESTAMPTZ,
    cooldown_until TIMESTAMPTZ,
    last_action_at TIMESTAMPTZ,
    last_before_bytes BIGINT,
    last_after_bytes BIGINT,
    consecutive_breaches INTEGER NOT NULL DEFAULT 0,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    target SMALLINT NOT NULL,
    last_action_result SMALLINT,
    is_action_pending BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT pk_postgresql_maintenance_targets PRIMARY KEY (tenant_key, target),
    CONSTRAINT fk_postgresql_maintenance_targets_tenant FOREIGN KEY (tenant_key)
        REFERENCES tenants(tenant_key) ON DELETE CASCADE,
    CONSTRAINT chk_postgresql_maintenance_targets_target
        CHECK (target BETWEEN 1 AND 12),
    CONSTRAINT chk_postgresql_maintenance_targets_last_action_result
        CHECK (last_action_result IS NULL OR last_action_result BETWEEN 1 AND 2),
    CONSTRAINT chk_postgresql_maintenance_targets_breaches_non_negative
        CHECK (consecutive_breaches >= 0),
    CONSTRAINT chk_postgresql_maintenance_targets_failures_non_negative
        CHECK (consecutive_failures >= 0),
    CONSTRAINT chk_postgresql_maintenance_targets_sizes_non_negative
        CHECK ((last_before_bytes IS NULL OR last_before_bytes >= 0)
            AND (last_after_bytes IS NULL OR last_after_bytes >= 0))
);

-- Tenant selection: never-inspected tenants first, then the least recently
-- inspected.
CREATE INDEX idx_postgresql_maintenance_tenants_last_inspected_at
    ON postgresql_maintenance_tenants (last_inspected_at NULLS FIRST, tenant_key);
