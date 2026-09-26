-- ============================================================================
-- 0005: Tenant wake schedule (platform database)
-- ============================================================================
-- When each tenant should next be loaded to run background work it has due
-- while no request is touching it -- a periodic notification, a durable-delivery
-- retry, a snapshot preparation, an expiry sweep. One broker replica evicts a
-- dormant tenant, probes its database for the earliest such instant, and records
-- it here; any replica may later claim the row and wake the tenant. The schedule
-- lives in the platform database, not in a broker's memory, so a wake survives
-- the eviction of the runtime that scheduled it and is not duplicated across
-- replicas.
--
-- `tenant_id` is the `tenants(id)` surrogate, cascade-deleted with the tenant so
-- a removed tenant leaves no orphan schedule. `wake_at` is the next instant the
-- tenant has work due, NULL when a probe found nothing (the tenant then stays
-- dormant until a later probe finds work). `probed_at` is when the schedule was
-- last written, which the crash-recovery reconciliation orders by so every
-- tenant is eventually re-probed.
--
-- The partial index on `wake_at` serves the claim -- `WHERE wake_at <= now ORDER
-- BY wake_at` -- and holds only the rows that have a wake pending, so it stays
-- small on a fleet where most tenants have nothing due. The `probed_at` index
-- serves the reconciliation's least-recently-probed ordering.
-- ============================================================================

CREATE TABLE tenant_wake_schedule (
    tenant_id BIGINT NOT NULL,
    wake_at TIMESTAMPTZ,
    probed_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT pk_tenant_wake_schedule PRIMARY KEY (tenant_id),
    CONSTRAINT fk_tenant_wake_schedule_tenant FOREIGN KEY (tenant_id)
        REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE INDEX idx_tenant_wake_schedule_wake_at
    ON tenant_wake_schedule (wake_at)
    WHERE wake_at IS NOT NULL;

CREATE INDEX idx_tenant_wake_schedule_probed_at
    ON tenant_wake_schedule (probed_at);
