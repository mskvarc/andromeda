-- ============================================================================
-- 0027: Subscription forwarding mappings
-- ============================================================================
-- A broker keeps two mappings for each local subscription that takes part in
-- distributed operations (clause 5.8.1.4): the companion context source
-- registration subscription that mirrors it, and the identifier of the reduced
-- copy placed at each matching registration's context source. A deletion tears
-- both down (clause 5.8.5.4); an inbound notification is routed by the copy's
-- identifier (clause 5.8.6).
--
-- These mappings are tenant-global, not branch state -- a Snapshot branch never
-- forwards -- so this file lives in `migrations/tenant/` and runs after the
-- branch set has created the live `subscriptions` it references. Identifiers
-- are `COLLATE "C"`, compared for equality only.
--
-- Every edge cascades. Deleting a subscription removes its companion mapping
-- (from either end) and its forwarded copies; deleting a registration removes
-- the copies held at its context source. The remote copies themselves are not
-- the database's to remove: the broker reads the mappings before it deletes
-- the owning row and issues the remote DELETEs from what it read, so the
-- cascade only ever clears rows whose teardown has already been captured.
-- `idx_forwarded_subscriptions_registration` backs the registration cascade
-- and the per-registration read that precedes it; the local-subscription
-- side is the primary key's leading column, and each companion column has its
-- own unique index.
-- ============================================================================

CREATE TABLE subscription_companions (
    -- The local entity subscription.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL PRIMARY KEY
        REFERENCES subscriptions(id) ON DELETE CASCADE,

    -- The companion context source registration subscription this broker minted.
    companion_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE
        REFERENCES subscriptions(id) ON DELETE CASCADE
);

CREATE TABLE forwarded_subscriptions (
    -- The local entity subscription the copy reduces.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL
        REFERENCES subscriptions(id) ON DELETE CASCADE,

    -- The registration whose context source holds the copy.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,

    -- The identifier the copy carries at the context source.
    remote_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    PRIMARY KEY (local_subscription_id, registration_id)
);

CREATE INDEX idx_forwarded_subscriptions_registration
    ON forwarded_subscriptions (registration_id);
