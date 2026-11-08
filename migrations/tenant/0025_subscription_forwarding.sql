-- ============================================================================
-- 0025: Subscription forwarding
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
-- Both ends of a companion mapping and the owner of a copy are held by their
-- `subscriptions` surrogate key, as every other relation of a subscription
-- holds it. The broker names a subscription by its URI and resolves the key in
-- the statement that reads or writes the mapping.
--
-- Every edge cascades: deleting a subscription removes its companion mapping,
-- from either end, and its forwarded copies; deleting a registration removes
-- the copies held at its context source. The remote copies are not the
-- database's to remove. The broker reads the mappings before it deletes the
-- owning row and issues the remote deletions from what it read, so a cascade
-- only clears rows whose teardown has already been captured.
-- ============================================================================
create table subscription_companions (
    local_subscription_key bigint not null,
    -- The context source registration subscription this broker minted.
    companion_subscription_key bigint not null,
    constraint pk_subscription_companions primary key (local_subscription_key),
    constraint uq_subscription_companions_companion unique (companion_subscription_key),
    constraint fk_subscription_companions_local foreign key (local_subscription_key) references subscriptions (subscription_key) on delete cascade,
    constraint fk_subscription_companions_companion foreign key (companion_subscription_key) references subscriptions (subscription_key) on delete cascade
);

create table forwarded_subscriptions (
    local_subscription_key bigint not null,
    -- The registration whose context source holds the copy.
    registration_id text compression lz4 collate "C" not null,
    -- The identifier the copy carries at the context source.
    remote_subscription_id text compression lz4 collate "C" not null,
    constraint pk_forwarded_subscriptions primary key (local_subscription_key, registration_id),
    constraint uq_forwarded_subscriptions_remote unique (remote_subscription_id),
    constraint fk_forwarded_subscriptions_local foreign key (local_subscription_key) references subscriptions (subscription_key) on delete cascade,
    constraint fk_forwarded_subscriptions_registration foreign key (registration_id) references context_source_registrations (id) on delete cascade,
    constraint chk_forwarded_subscriptions_registration_id_length check (octet_length(registration_id) <= 1024),
    constraint chk_forwarded_subscriptions_remote_subscription_id_length check (octet_length(remote_subscription_id) <= 1024)
);

-- The registration cascade, and the per-registration read that precedes it.
create index idx_forwarded_subscriptions_registration on forwarded_subscriptions (registration_id);
