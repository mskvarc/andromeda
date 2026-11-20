-- ============================================================================
-- 0040: Subscription receiver redirects
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.8.1.4, 5.8.6
--
-- Every companion and every copy placed at a context source notifies the
-- receiver below the origin the broker advertises. That receiver changes
-- when the operator advertises another origin, or when a release moves the
-- receiver's path, and a companion or copy placed before the change keeps
-- notifying a receiver the broker no longer serves.
--
-- The receiver the broker adopted last is the one row of
-- `subscription_receiver`. Before a tenant serves a request, the broker
-- adopts the receiver it serves, and when that differs from the one held
-- here, every companion and every settled copy owes it. The table starts
-- empty, so a tenant's first adoption owes the receiver to every companion
-- and copy placed before it.
--
-- A companion owes the receiver while `subscription_companion_redirects`
-- holds a row for it, and a copy while `forwarded_subscription_redirects`
-- holds one. Each row is keyed like what it redirects and removed with it, as
-- a copy's claim is, and holds the instant the update is next due. The
-- tenant's background pass updates each one due and removes its row, or
-- moves the instant on when the source gives no verdict.
-- ============================================================================
create table subscription_receiver (
    singleton boolean not null default true,
    -- The receiver every companion and copy notifies, except one that still
    -- owes the update.
    receiver text compression lz4 collate "C" not null,
    constraint pk_subscription_receiver primary key (singleton),
    constraint chk_subscription_receiver_singleton check (singleton)
);

create table subscription_companion_redirects (
    local_subscription_key bigint not null,
    -- The instant the companion's update to the current receiver is next due.
    redirect_due_at timestamptz not null,
    constraint pk_subscription_companion_redirects primary key (local_subscription_key),
    constraint fk_subscription_companion_redirects_companion foreign key (local_subscription_key) references subscription_companions (local_subscription_key) on delete cascade
);

create table forwarded_subscription_redirects (
    local_subscription_key bigint not null,
    -- The instant the copy's update to the current receiver is next due.
    redirect_due_at timestamptz not null,
    -- The registration whose context source holds the copy.
    registration_id text compression lz4 collate "C" not null,
    constraint pk_forwarded_subscription_redirects primary key (local_subscription_key, registration_id),
    constraint fk_forwarded_subscription_redirects_copy foreign key (local_subscription_key, registration_id) references forwarded_subscriptions (local_subscription_key, registration_id) on delete cascade,
    constraint chk_forwarded_subscription_redirects_registration_id_length check (octet_length(registration_id) <= 1024)
);

-- The obligation read and the background pass read the updates due, the
-- earliest first.
create index idx_subscription_companion_redirects_redirect_due_at on subscription_companion_redirects (redirect_due_at);

create index idx_forwarded_subscription_redirects_redirect_due_at on forwarded_subscription_redirects (redirect_due_at);
