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
-- are `COLLATE "C"`, compared for equality only, and bounded at 1024 UTF-8
-- octets, the identifier bound (`IDENTIFIER_BYTE_LIMIT` in
-- `andromeda-ngsi-ld-lexicon`) each is parsed under -- chosen so the two
-- identifiers of the `forwarded_subscriptions` primary key fit one btree
-- entry, which a btree refuses past 2704 bytes.
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
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The companion context source registration subscription this broker minted.
    companion_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    CONSTRAINT pk_subscription_companions PRIMARY KEY (local_subscription_id),
    CONSTRAINT uq_subscription_companions_companion UNIQUE (companion_subscription_id),
    CONSTRAINT fk_subscription_companions_local FOREIGN KEY (local_subscription_id)
        REFERENCES subscriptions(id) ON DELETE CASCADE,
    CONSTRAINT fk_subscription_companions_companion FOREIGN KEY (companion_subscription_id)
        REFERENCES subscriptions(id) ON DELETE CASCADE,
    CONSTRAINT chk_subscription_companions_local_subscription_id_length
        CHECK (octet_length(local_subscription_id) <= 1024),
    CONSTRAINT chk_subscription_companions_companion_subscription_id_length
        CHECK (octet_length(companion_subscription_id) <= 1024)
);

CREATE TABLE forwarded_subscriptions (
    -- The local entity subscription the copy reduces.
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The registration whose context source holds the copy.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The identifier the copy carries at the context source.
    remote_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    CONSTRAINT pk_forwarded_subscriptions PRIMARY KEY (local_subscription_id, registration_id),
    CONSTRAINT uq_forwarded_subscriptions_remote UNIQUE (remote_subscription_id),
    CONSTRAINT fk_forwarded_subscriptions_local FOREIGN KEY (local_subscription_id)
        REFERENCES subscriptions(id) ON DELETE CASCADE,
    CONSTRAINT fk_forwarded_subscriptions_registration FOREIGN KEY (registration_id)
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    CONSTRAINT chk_forwarded_subscriptions_local_subscription_id_length
        CHECK (octet_length(local_subscription_id) <= 1024),
    CONSTRAINT chk_forwarded_subscriptions_registration_id_length
        CHECK (octet_length(registration_id) <= 1024),
    CONSTRAINT chk_forwarded_subscriptions_remote_subscription_id_length
        CHECK (octet_length(remote_subscription_id) <= 1024)
);

CREATE INDEX idx_forwarded_subscriptions_registration
    ON forwarded_subscriptions (registration_id);
