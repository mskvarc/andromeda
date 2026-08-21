-- ============================================================================
-- 1003: Forwarded subscription routes (platform database)
-- ============================================================================
-- Every subscription identifier this broker mints and hands to a peer — the
-- companion registration subscription of a local subscription, and each
-- reduced copy placed at a context source (clause 5.8.1.4) — mapped to the
-- tenant that owns it. An inbound notification names the minted identifier
-- and carries, at most, the peer's own tenant (clause 6.3.14), so this table
-- is how the receiver finds the tenant database to continue in. NULL names the
-- unnamed default tenant.
-- ============================================================================

CREATE TABLE IF NOT EXISTS forwarded_subscription_routes (
    minted_subscription_id TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    tenant TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
