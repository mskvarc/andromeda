-- ============================================================================
-- 0004: Tenant wake schedule
-- ============================================================================
-- When each dormant tenant next has background work due while no request is
-- touching it: a periodic notification, a durable-delivery retry, a Snapshot
-- preparation, an expiry sweep. The replica that evicts a tenant probes its
-- database for the earliest such instant and records it here, and any replica
-- may later claim the row and wake the tenant. Kept in the platform database
-- rather than in a broker's memory, a wake survives the eviction of the
-- runtime that scheduled it and is never duplicated across replicas.
-- ============================================================================
create table tenant_wake_schedule (
    tenant_key bigint not null,
    -- NULL when the last probe found no work; the tenant then stays dormant
    -- until a later probe finds some.
    wake_at timestamptz,
    -- When the row was last written. Crash-recovery reconciliation re-probes
    -- the least recently probed tenants first, so every tenant is eventually
    -- re-probed. No index serves that order: the sweep reads every tenant
    -- beside its row, those without one first, which an index on this column
    -- cannot order.
    probed_at timestamptz not null,
    constraint pk_tenant_wake_schedule primary key (tenant_key),
    constraint fk_tenant_wake_schedule_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade
);

-- The wake claim: due tenants in `wake_at` order. Only pending wakes are
-- indexed, so it stays small when most tenants have nothing due.
create index idx_tenant_wake_schedule_wake_at on tenant_wake_schedule (wake_at)
where
    wake_at is not null;
