-- ============================================================================
-- 0027: Subscription forwarding
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.8.1.4, 5.8.5.4, 5.8.6
--
-- The two mappings kept for each local subscription that takes part in
-- distributed operations (clause 5.8.1.4): the companion context source
-- registration subscription that mirrors it, and the reduced copy placed at
-- each matching registration's context source. Deleting the subscription
-- tears both down (clause 5.8.5.4), and an inbound notification is routed by
-- the copy's identifier (clause 5.8.6).
--
-- Every edge cascades: deleting a subscription removes its companion mapping,
-- from either end, and its forwarded copies; deleting a registration removes
-- the copies held at its context source. The remote copies are not the
-- database's to remove. The broker reads the mappings before it deletes the
-- owning row and issues the remote deletions from what it read, so a cascade
-- only clears rows whose teardown has already been captured.
-- ============================================================================

CREATE TABLE subscription_companions (
    local_subscription_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- The context source registration subscription this broker minted.
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

-- The registration cascade, and the per-registration read that precedes it.
CREATE INDEX idx_forwarded_subscriptions_registration
    ON forwarded_subscriptions (registration_id);
