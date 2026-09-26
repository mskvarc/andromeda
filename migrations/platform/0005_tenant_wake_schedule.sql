-- ============================================================================
-- 0005: Tenant wake schedule
-- ============================================================================
-- When each dormant tenant next has background work due while no request is
-- touching it: a periodic notification, a durable-delivery retry, a Snapshot
-- preparation, an expiry sweep. The replica that evicts a tenant probes its
-- database for the earliest such instant and records it here, and any replica
-- may later claim the row and wake the tenant. Kept in the platform database
-- rather than in a broker's memory, a wake survives the eviction of the
-- runtime that scheduled it and is never duplicated across replicas.
-- ============================================================================

CREATE TABLE tenant_wake_schedule (
    tenant_key BIGINT NOT NULL,
    -- NULL when the last probe found no work; the tenant then stays dormant
    -- until a later probe finds some.
    wake_at TIMESTAMPTZ,
    -- When the row was last written. Crash-recovery reconciliation re-probes
    -- the least recently probed tenants first, so every tenant is eventually
    -- re-probed.
    probed_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT pk_tenant_wake_schedule PRIMARY KEY (tenant_key),
    CONSTRAINT fk_tenant_wake_schedule_tenant FOREIGN KEY (tenant_key)
        REFERENCES tenants(tenant_key) ON DELETE CASCADE
);

-- The wake claim: due tenants in `wake_at` order. Only pending wakes are
-- indexed, so it stays small when most tenants have nothing due.
CREATE INDEX idx_tenant_wake_schedule_wake_at
    ON tenant_wake_schedule (wake_at)
    WHERE wake_at IS NOT NULL;

-- Reconciliation's least-recently-probed ordering.
CREATE INDEX idx_tenant_wake_schedule_probed_at
    ON tenant_wake_schedule (probed_at);
