-- ============================================================================
-- 0031: Forwarded subscription placement
-- ============================================================================
-- A copy of a local subscription is recorded before it is sent to the context
-- source, so a notification the source sends under it at once is routed back
-- (ETSI GS CIM 009 v1.9.1 clauses 5.8.1.4 and 5.8.6). The placement that
-- recorded it can be cut off before the source has answered, by the request
-- deadline, a shutdown or a crash, and the record then names a copy the source
-- may never have received.
--
-- A claim row tells a placement still under way from an abandoned one. It is
-- written with the copy's record and holds the instant the placement's lease
-- ends. The source's answer settles the copy and deletes the row, when the
-- source accepted the copy or answered that it already holds a subscription
-- under its identifier. A claim past its lease was abandoned. The next
-- placement of the same copy takes it over and sends the copy again under the
-- identifier recorded, and the tenant's background resumption does the same
-- for every abandoned claim, so a source ends up holding the copy whatever
-- point a placement was cut off at.
--
-- A copy recorded without a claim row is settled, which is every copy the
-- broker recorded before. Removing the copy's record removes its claim.
-- ============================================================================
create table forwarded_subscription_claims (
    local_subscription_key bigint not null,
    -- The instant the lease of the placement holding the claim ends.
    lease_until timestamptz not null,
    -- The registration whose context source the copy is placed at.
    registration_id text compression lz4 collate "C" not null,
    constraint pk_forwarded_subscription_claims primary key (local_subscription_key, registration_id),
    constraint fk_forwarded_subscription_claims_copy foreign key (local_subscription_key, registration_id) references forwarded_subscriptions (local_subscription_key, registration_id) on delete cascade,
    constraint chk_forwarded_subscription_claims_registration_id_length check (octet_length(registration_id) <= 1024)
);

-- The resumption's read of abandoned claims, oldest lease first, and the
-- earliest obligation of a tenant nobody is using.
create index idx_forwarded_subscription_claims_lease_claim on forwarded_subscription_claims (lease_until);
