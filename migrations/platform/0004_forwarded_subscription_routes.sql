-- ============================================================================
-- 0004: Forwarded subscription routes
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.8.1.4, 6.3.14
--
-- Every subscription identifier this broker mints and hands to a peer, mapped
-- to the tenant that owns it: the companion registration subscription of a
-- local subscription, and each reduced copy placed at a context source
-- (clause 5.8.1.4). An inbound notification names only the minted identifier
-- and, at most, the peer's own tenant (clause 6.3.14), so this table is how
-- the receiving broker finds the tenant database to continue in. The unnamed
-- default tenant has routes like any other.
-- ============================================================================

CREATE TABLE forwarded_subscription_routes (
    tenant_key BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    minted_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_forwarded_subscription_routes PRIMARY KEY (minted_subscription_id),
    CONSTRAINT fk_forwarded_subscription_routes_tenant FOREIGN KEY (tenant_key)
        REFERENCES tenants(tenant_key) ON DELETE CASCADE,
    CONSTRAINT chk_forwarded_subscription_routes_minted_id_length
        CHECK (octet_length(minted_subscription_id) <= 1024)
);

CREATE INDEX idx_forwarded_subscription_routes_tenant
    ON forwarded_subscription_routes (tenant_key);
