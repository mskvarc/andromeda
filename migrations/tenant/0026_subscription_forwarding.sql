-- ============================================================================
-- 0026: Subscription forwarding mappings
-- ============================================================================
-- Clause 5.8.1.4 has a broker keep two mappings for each local subscription
-- that takes part in distributed operations: the companion context source
-- registration subscription that mirrors it (`subscription_companions`), and
-- the identifier of the reduced copy placed at each matching registration's
-- context source (`forwarded_subscriptions`). A deletion tears both down
-- (clause 5.8.5.4); an inbound notification is routed by the copy's identifier
-- (clause 5.8.6). Identifiers are `COLLATE "C"`: compared for equality only.
-- ============================================================================

CREATE TABLE subscription_companions (
    -- The local entity subscription.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL PRIMARY KEY,

    -- The companion context source registration subscription this broker minted.
    companion_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE
);

CREATE TABLE forwarded_subscriptions (
    -- The local entity subscription the copy reduces.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The registration whose context source holds the copy.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The identifier the copy carries at the context source.
    remote_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    PRIMARY KEY (local_subscription_id, registration_id)
);
