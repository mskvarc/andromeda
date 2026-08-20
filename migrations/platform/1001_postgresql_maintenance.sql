-- ============================================================================
-- 1001: Process-level PostgreSQL maintenance state
-- ============================================================================
-- The controller is scheduled by every broker replica, but the platform
-- database is the shared coordination point. Tenant inspection timestamps make
-- selection fair across restarts and include tenants whose runtime is absent.
-- Target rows retain the evidence needed to avoid acting on one noisy sample.
-- ============================================================================

CREATE TABLE postgresql_maintenance_tenants (
    tenant_id BIGINT PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    last_inspected_at TIMESTAMPTZ,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_result TEXT,
    CONSTRAINT chk_postgresql_maintenance_tenant_failures_non_negative
        CHECK (consecutive_failures >= 0)
);

CREATE TABLE postgresql_maintenance_targets (
    tenant_id BIGINT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    last_sampled_at TIMESTAMPTZ,
    consecutive_breaches INTEGER NOT NULL DEFAULT 0,
    pending_action BOOLEAN NOT NULL DEFAULT FALSE,
    cooldown_until TIMESTAMPTZ,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_action_at TIMESTAMPTZ,
    last_action_result TEXT,
    last_before_bytes BIGINT,
    last_after_bytes BIGINT,
    PRIMARY KEY (tenant_id, target),
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
