-- ============================================================================
-- 0004: Forwarded subscription routes (platform database)
-- ============================================================================
-- Every subscription identifier this broker mints and hands to a peer -- the
-- companion registration subscription of a local subscription, and each
-- reduced copy placed at a context source (clause 5.8.1.4) -- mapped to the
-- tenant that owns it. An inbound notification names the minted identifier
-- and carries, at most, the peer's own tenant (clause 6.3.14), so this table
-- is how the receiver finds the tenant database to continue in.
--
-- `tenant_id` is the `tenants(id)` surrogate, the unnamed default tenant
-- included, and is cascade-deleted with the tenant so a removed tenant leaves
-- no route pointing at a database that no longer exists.
-- `idx_forwarded_subscription_routes_tenant` backs that cascade.
-- ============================================================================

CREATE TABLE forwarded_subscription_routes (
    tenant_id BIGINT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    minted_subscription_id TEXT COLLATE "C" NOT NULL PRIMARY KEY
);

CREATE INDEX idx_forwarded_subscription_routes_tenant
    ON forwarded_subscription_routes (tenant_id);
