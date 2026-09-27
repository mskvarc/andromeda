-- ============================================================================
-- 0030: Registrations notified to context source subscriptions
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.3.3, 5.11.2.4, 5.11.3.4, 5.11.7
--
-- The context source registrations each context source registration
-- subscription has been sent as matching and not since as no longer matching.
-- Clause 5.3.3 decides every trigger reason against that set: `newlyMatching`
-- reports a registration the subscriber has not been told matches, while
-- `updated` and `noLongerMatching` concern one that "was part of a previous
-- notification".
--
-- Inserting a row claims the `newlyMatching` notification of its registration
-- and deleting one claims the `noLongerMatching` notification, so whichever
-- broker evaluates a change first sends it, and a subscription's initial
-- notification and a registration change evaluated at the same moment never
-- both report one registration.
-- ============================================================================

CREATE TABLE subscription_notified_registrations (
    subscription_key BIGINT NOT NULL,
    -- References no registration: the row outlives the registration's
    -- deletion until the `noLongerMatching` notification it owes is claimed.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_subscription_notified_registrations
        PRIMARY KEY (subscription_key, registration_id),
    CONSTRAINT fk_subscription_notified_registrations_subscription
        FOREIGN KEY (subscription_key)
        REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    CONSTRAINT chk_subscription_notified_registrations_registration_id_length
        CHECK (octet_length(registration_id) <= 1024)
);

-- The subscriptions a changed registration was notified to, read before each
-- withdrawal.
CREATE INDEX idx_subscription_notified_registrations_registration
    ON subscription_notified_registrations (registration_id);
