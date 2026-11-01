-- ============================================================================
-- 0023: Notification queue
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.2.12, 5.2.14.1, 5.6.6, 5.8.6
--
-- The durable notification queue, as two relations: the payload belongs to
-- the change, and only the queue state belongs to the delivery. An Entity
-- watched by twenty Subscriptions produces one event and twenty deliveries.
-- The Entity's state and, under `showChanges` (clause 5.2.14.1), its previous
-- state are written once per change, while every claim, retry and
-- acknowledgement rewrites a narrow delivery row.
--
-- One event records one write's change to one Entity, which clause 5.8.6
-- notifies once per Subscription: the Entity's lifecycle transition and every
-- Attribute instance the write created, updated or deleted, as three parallel
-- arrays with one element per instance.
--
-- Codes:
--
--   lifecycle: 1 created, 2 modified, 3 deleted
--   change_kinds: 1 created, 2 updated, 3 deleted
--
-- An event that no Subscription matches is never written, so no stored event
-- is without a delivery until its deliveries have been reaped.
-- ============================================================================
-- One row per change, written in the Entity's own transaction and never
-- updated. Claims never touch it, so its payloads are read from a heap no
-- queue traffic churns. It is reached only by primary key from a claimed
-- delivery, or by write identity when a replay checks whether its events are
-- already there.
create table notification_events (
    event_key bigint generated always as identity (cache 32),
    created_at timestamptz not null,
    -- The store's transaction timestamp of the commit that made the change:
    -- the inserting transaction's own, or for a replayed job the timestamp of
    -- the commit that queued it. A delivery is dropped when its Subscription
    -- last became active after this instant, so a change committed while the
    -- Subscription was paused is never notified once it is active again
    -- (clause 5.8.6). Both instants come from the same clock whichever broker
    -- wrote them.
    committed_at timestamptz not null,
    -- The clause 4.8 createdAt, modifiedAt and expiresAt of an Entity a
    -- deletion removed. By delivery the Entity's row is gone, so clause 5.8.6
    -- can only report them from here; a change that leaves the Entity in place
    -- stores none and the delivery reads them from the Entity.
    entity_created_at timestamptz,
    entity_modified_at timestamptz,
    entity_expires_at timestamptz,
    -- The event's deterministic position within its write. With `write_id`
    -- it makes a replayed write insert nothing rather than announce a change
    -- a second time.
    event_ordinal integer not null,
    lifecycle smallint not null,
    write_id uuid not null,
    -- No foreign key: clause 5.6.6 deletion notifies about an Entity that no
    -- longer exists, and a reference would either forbid that row or cascade
    -- away the announcement of the deletion.
    entity_id text compression lz4 collate "C" not null,
    -- One element per changed Attribute instance, position for position:
    -- its transition, its `attribute_names` key and its `datasetId` (NULL
    -- for the default instance). Empty, never NULL, for a change that
    -- touched no instance: an Entity created or deleted with no Attribute.
    change_kinds smallint[] compression lz4 not null,
    change_attribute_name_keys integer[] compression lz4 not null,
    change_dataset_ids text[] compression lz4 collate "C" not null,
    -- The Entity's state after the change; for a deletion, the state it
    -- removed, which is also its previous state, so a deletion stores no
    -- `previous_snapshot`.
    entity_snapshot entity_state_record compression lz4,
    -- The Entity's state before a change that left it in place.
    previous_snapshot entity_state_record compression lz4,
    constraint pk_notification_events primary key (event_key),
    constraint uq_notification_events_write_identity unique (write_id, event_ordinal),
    constraint chk_notification_events_event_ordinal check (event_ordinal >= 0),
    constraint chk_notification_events_lifecycle check (lifecycle between 1 and 3),
    constraint chk_notification_events_change_kinds check (change_kinds <@ array[1, 2, 3]::smallint[]),
    constraint chk_notification_events_change_arity check (cardinality(change_attribute_name_keys) = cardinality(change_kinds) and cardinality(change_dataset_ids) = cardinality(change_kinds)),
    constraint chk_notification_events_deletion_state_once check (lifecycle <> 3 or previous_snapshot is null),
    constraint chk_notification_events_entity_system_times check (num_nonnulls(entity_created_at, entity_modified_at) in (0, 2) and (entity_expires_at is null or entity_created_at is not null)),
    constraint chk_notification_events_entity_id_length check (octet_length(entity_id) <= 1024)
)
with (
    vacuum_truncate = false,
    autovacuum_vacuum_insert_scale_factor = 0,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The queue state, one row per matched Subscription, and the only relation a
-- claim scans, locks or updates.
create table notification_deliveries (
    delivery_key bigint generated always as identity (cache 32),
    event_key bigint not null,
    subscription_key bigint not null,
    next_attempt_at timestamptz not null,
    attempt_count integer not null default 0,
    claimed_at timestamptz,
    lease_until timestamptz,
    delivered_at timestamptz,
    exhausted_at timestamptz,
    -- The instant the delivery was set aside behind the probe of its
    -- Subscription's hold, NULL while it waits its turn in the claim order. A
    -- held delivery leaves the claim index, so a backlog behind an endpoint
    -- that fails costs a claim nothing, and it keeps its attempt count and its
    -- due instant for when the hold is released.
    held_at timestamptz,
    claimed_by uuid,
    last_error text compression lz4 collate "C",
    constraint pk_notification_deliveries primary key (delivery_key),
    constraint fk_notification_deliveries_event foreign key (event_key) references notification_events (event_key) on delete cascade,
    constraint fk_notification_deliveries_subscription foreign key (subscription_key) references subscriptions (subscription_key) on delete cascade,
    -- Clause 5.8.6 notifies a change once per Subscription, so a change owes
    -- each Subscription one delivery however often its commit runs. Led by
    -- `event_key`, its index also serves the event cascade and the check the
    -- reap runs for events whose last delivery it removed.
    constraint uq_notification_deliveries_event_subscription unique (event_key, subscription_key),
    constraint chk_notification_deliveries_attempt_count_non_negative check (attempt_count >= 0),
    constraint chk_notification_deliveries_claim_coherent check (num_nonnulls(claimed_at, claimed_by, lease_until) in (0, 3)),
    constraint chk_notification_deliveries_exhausted_state check (exhausted_at is null or (delivered_at is null and num_nonnulls(claimed_at, claimed_by, lease_until) = 0)),
    constraint chk_notification_deliveries_held_state check (held_at is null or (delivered_at is null and exhausted_at is null and num_nonnulls(claimed_at, claimed_by, lease_until) = 0)),
    constraint chk_notification_deliveries_last_error_length check (last_error is null or char_length(last_error) <= 2048)
)
with (
    vacuum_truncate = false,
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim: rows still in the queue and not held behind a probe, in the
-- order they became claimable. The expression is the later of when a row is
-- due and when its lease lapses, so a claim is one range scan as deep as its
-- batch.
create index idx_notification_deliveries_claim on notification_deliveries (greatest(next_attempt_at, coalesce(lease_until, '-infinity'::timestamptz)), delivery_key)
where
    delivered_at is null and exhausted_at is null and held_at is null;

-- The reap of delivered rows.
create index idx_notification_deliveries_delivered_at on notification_deliveries (delivered_at)
where
    delivered_at is not null;

-- The reap of dead-lettered rows.
create index idx_notification_deliveries_exhausted_at on notification_deliveries (exhausted_at)
where
    exhausted_at is not null;

-- One Subscription's deliveries: the cascade of its deletion, the rows a hold
-- sets aside (`held_at` NULL) and releases (`held_at` set), and the held row
-- that probes next, the one set aside first.
create index idx_notification_deliveries_subscription on notification_deliveries (subscription_key, held_at, delivery_key);

-- One row per Subscription whose endpoint failed its last attempt and has not
-- answered since. Its probe is the one delivery of the Subscription a claim may
-- take; every other one is held (`held_at`) until an attempt to the endpoint
-- succeeds, which deletes the hold and releases them. An endpoint that never
-- answers therefore costs one slot per retry step, however long its backlog,
-- instead of every slot of the tenant's drain for one `request_timeout` after
-- another.
--
-- The probe reference has no action of its own: a probe is always still
-- queued, because the attempt that delivers it deletes the hold and the
-- attempt that exhausts it hands the hold to the next held delivery in the
-- same statement, so only the Subscription's deletion removes it, and that
-- cascade removes the hold with it.
create table notification_delivery_holds (
    subscription_key bigint not null,
    -- The probe: the one delivery of the Subscription a claim may take.
    delivery_key bigint not null,
    constraint pk_notification_delivery_holds primary key (subscription_key),
    constraint uq_notification_delivery_holds_delivery unique (delivery_key),
    constraint fk_notification_delivery_holds_subscription foreign key (subscription_key) references subscriptions (subscription_key) on delete cascade,
    constraint fk_notification_delivery_holds_delivery foreign key (delivery_key) references notification_deliveries (delivery_key)
)
with (
    vacuum_truncate = false,
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);
