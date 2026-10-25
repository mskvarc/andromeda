-- ============================================================================
-- 0029: Registrations notified to context source subscriptions
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
create table subscription_notified_registrations (
    subscription_key bigint not null,
    -- References no registration: the row outlives the registration's
    -- deletion until the `noLongerMatching` notification it owes is claimed.
    registration_id text compression lz4 collate "C" not null,
    constraint pk_subscription_notified_registrations primary key (subscription_key, registration_id),
    constraint fk_subscription_notified_registrations_subscription foreign key (subscription_key) references subscriptions (subscription_key) on delete cascade,
    constraint chk_subscription_notified_registrations_registration_id_length check (octet_length(registration_id) <= 1024)
);

-- The subscriptions a changed registration was notified to, read before each
-- withdrawal.
create index idx_subscription_notified_registrations_registration on subscription_notified_registrations (registration_id);
