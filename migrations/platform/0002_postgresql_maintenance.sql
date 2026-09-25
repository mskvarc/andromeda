-- ============================================================================
-- 0002: Process-level PostgreSQL maintenance state (platform database)
-- ============================================================================
-- The controller is scheduled by every broker replica, but the platform
-- database is the shared coordination point. Tenant inspection timestamps make
-- selection fair across restarts and include tenants whose runtime is absent.
-- Target rows retain the evidence needed to avoid acting on one noisy sample.
--
-- Every closed vocabulary here is a code of a migration-owned registry the
-- broker compiles in (`andromeda-postgresql-codes`), append-only and never
-- renumbered; each check pins its column to the registered range.
--
--   last_result (`MaintenanceInspectionResultCode`):
--     1 started, 2 unavailable, 3 ready, 4 missing-pgstattuple,
--     5 missing-pgstattuple-execute, 6 missing-maintain, 7 disabled,
--     8 global-lock-unavailable, 9 no-tenant, 10 tenant-unavailable,
--     11 schema-behind, 12 capability-unavailable, 13 idle, 14 probed,
--     15 queued, 16 would-act, 17 deferred, 18 acted, 19 action-failed,
--     20 cancelled
--   target (`MaintenanceTargetCode`):
--     1 uq_current_attribute_values_grain, 2 idx_cav_attr_number,
--     3 idx_cav_attr_string, 4 idx_cav_attr_string_trgm, 5 idx_cav_attr_bool,
--     6 idx_cav_attr_datetime, 7 idx_cav_attr_uri, 8 idx_cav_attr_geo,
--     9 idx_cav_attr_entity_geo, 10 current_attribute_values,
--     11 entity_documents, 12 subscription_delivery_state
--   last_action_result (`MaintenanceActionResultCode`):
--     1 succeeded, 2 failed
-- ============================================================================

CREATE TABLE postgresql_maintenance_tenants (
    tenant_id BIGINT PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    last_inspected_at TIMESTAMPTZ,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_result SMALLINT,
    CONSTRAINT chk_postgresql_maintenance_tenant_failures_non_negative
        CHECK (consecutive_failures >= 0),
    CONSTRAINT chk_postgresql_maintenance_tenant_last_result
        CHECK (last_result IS NULL OR last_result BETWEEN 1 AND 20)
);

CREATE TABLE postgresql_maintenance_targets (
    tenant_id BIGINT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    last_sampled_at TIMESTAMPTZ,
    cooldown_until TIMESTAMPTZ,
    last_action_at TIMESTAMPTZ,
    last_before_bytes BIGINT,
    last_after_bytes BIGINT,
    consecutive_breaches INTEGER NOT NULL DEFAULT 0,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    target SMALLINT NOT NULL,
    last_action_result SMALLINT,
    pending_action BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, target),
    CONSTRAINT chk_postgresql_maintenance_target_code
        CHECK (target BETWEEN 1 AND 12),
    CONSTRAINT chk_postgresql_maintenance_target_action_result
        CHECK (last_action_result IS NULL OR last_action_result BETWEEN 1 AND 2),
    CONSTRAINT chk_postgresql_maintenance_target_breaches_non_negative
        CHECK (consecutive_breaches >= 0),
    CONSTRAINT chk_postgresql_maintenance_target_failures_non_negative
        CHECK (consecutive_failures >= 0),
    CONSTRAINT chk_postgresql_maintenance_target_sizes_non_negative
        CHECK ((last_before_bytes IS NULL OR last_before_bytes >= 0)
            AND (last_after_bytes IS NULL OR last_after_bytes >= 0))
);

CREATE INDEX idx_postgresql_maintenance_tenants_inspected
    ON postgresql_maintenance_tenants (last_inspected_at NULLS FIRST, tenant_id);
