-- ============================================================================
-- 0061: Split the notification queue into events and deliveries
-- ============================================================================
-- `notification_outbox` stored one row per (matched Subscription, changed
-- Entity), and each of those rows carried a full copy of the Entity snapshot
-- and, under `showChanges` (clause 5.5.12), of the previous one as well. An
-- Entity watched by twenty Subscriptions therefore wrote twenty copies of the
-- same two JSONB documents per change, updated one of them per delivery
-- attempt, and rewrote the whole wide tuple -- payload included -- on every
-- claim, failure and acknowledgement. The payload is a property of the change;
-- only the queue state is a property of the delivery.
--
-- `notification_events` holds the change, once. It is written inside the
-- Entity's own transaction and never updated afterwards: a claim, a retry and
-- an acknowledgement do not touch it, so the snapshots are read from a heap
-- that no queue traffic churns.
--
-- `notification_deliveries` holds the queue state, one row per matched
-- Subscription, and is the only relation the claim scans, locks or updates.
-- Its rows are narrow and fixed-width apart from two nullable diagnostics, so a
-- claim rewrites tens of bytes instead of kilobytes.
--
-- An event with no matching Subscription is never written. The writer drops it
-- before the statement runs, so the schema never holds a payload no delivery
-- references; the reap below is for events whose deliveries have since been
-- removed, not for events that never owed one.
--
-- What the columns say, and why they are not what the outbox held:
--
--   * `write_id` and `event_ordinal` are the write identity 0037 named,
--     carried here so a notification can be correlated with the history rows
--     and side effects of the same atomic write. `UNIQUE (write_id,
--     event_ordinal)` is what makes a replayed commit re-insert nothing rather
--     than emit a second notification for a change already announced; the
--     ordinal is the event's deterministic position within its write, so the
--     identity is stable across replays rather than dependent on insertion
--     order.
--   * `trigger` is a `SMALLINT` from the migration-owned registry the broker
--     compiles in (`andromeda-postgresql-codes`
--     `NotificationTriggerCode`), not the clause-5.3.3 token as text:
--
--       1 entityCreated, 2 entityUpdated, 3 entityDeleted,
--       4 attributeCreated, 5 attributeUpdated, 6 attributeDeleted
--
--     The check pins the range. A row carrying a number outside it could only
--     come from a release that knows a condition this schema does not, and the
--     reader refuses such a row by name rather than mapping it to a wrong
--     trigger.
--   * `changed_attribute_name_ids` are the four-byte dictionary keys 0035
--     introduced, not the expanded IRIs. A notification names a handful of
--     attributes out of a vocabulary the dictionary already stores once; the
--     claim resolves them back through `attribute_names` when it reads the
--     batch. The array is NOT NULL and empty for an entity-level trigger,
--     because "no attribute in particular" and "unknown" are not the same
--     statement and only one of them is true here.
--   * `entity_id` is `TEXT COLLATE "C"`, matching what 0056 made `entities.id`.
--     There is deliberately no foreign key: clause 5.6.6 deletion emits a
--     notification about an Entity that no longer exists, and a referential
--     edge would either forbid that row or cascade away the announcement of the
--     deletion itself.
--
-- Column order is this schema's alignment order -- not-null fixed-width by
-- descending alignment, then nullable fixed-width, then varlena, LZ4 on each
-- varlena column. `write_id` is a UUID, which PostgreSQL stores byte-aligned,
-- so it sorts after the two-byte trigger rather than with the eight-byte
-- columns its width would suggest.
--
-- Index set, one named consumer each:
--
--   * `idx_notification_deliveries_claim_candidates` is the claim's access
--     path. It is led by `next_attempt_at`, which is both the ordering and the
--     due test, carries `lease_until` so the lease test is answered from the
--     index rather than by a heap visit per candidate, and repeats the attempt
--     ceiling as the literal 5 in its predicate exactly as the outbox index did
--     since 0013. A partial index only serves a query whose predicate implies
--     its own, so the configured `notification_delivery.max_attempts` must
--     remain 5; raising it makes this index unusable rather than incorrect.
--     The claim statement's predicate is written to match this one clause for
--     clause.
--   * `idx_notification_deliveries_delivered` serves the cleanup's range scan
--     over completed rows, for the reason 0018 recorded: without it the reap
--     sequentially scans a table whose row count is dominated by delivered rows
--     until it catches up.
--   * `idx_notification_deliveries_subscription` supports the cascade. Deleting
--     a Subscription (clause 5.8.4) reclaims its queued deliveries in the same
--     statement, and PostgreSQL has no implicit index on the referencing side
--     of a foreign key.
--   * `idx_notification_deliveries_event` supports the orphan check the reap
--     runs against each event whose last delivery it removed.
--
-- No index on `notification_events` beyond its key and the write identity. It
-- is only ever reached by primary key from a claimed delivery, or by the write
-- identity when a replay tests whether its events are already there.
--
-- Storage parameters. `notification_deliveries` is queue-shaped: high insert
-- rate, every row updated once or twice, every row eventually deleted, so it
-- gets 0038's queue settings -- an absolute-ish autovacuum threshold, a small
-- scale factor, no vacuum cost delay -- and a fillfactor below 100.
-- `fillfactor` is the weaker lever, as 0038 recorded: claim and retry updates
-- write `next_attempt_at`, `delivered_at` and `lease_until`, which appear in
-- the claim index and its predicate, so those updates rarely qualify as
-- heap-only. `notification_events` is never updated and so takes no
-- fillfactor, but it is deleted in bulk by the reap and gets the same vacuum
-- cadence for that churn.
--
-- Every relation here is created empty, so the indexes are built in this
-- migration's transaction rather than concurrently: there is nothing to lock
-- out and no scan to keep short.
-- ============================================================================

CREATE TABLE notification_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    event_ordinal INTEGER NOT NULL,
    trigger SMALLINT NOT NULL,
    write_id UUID NOT NULL,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    changed_attribute_name_ids INTEGER[] COMPRESSION lz4 NOT NULL,
    entity_snapshot JSONB COMPRESSION lz4,
    previous_snapshot JSONB COMPRESSION lz4,
    CONSTRAINT uq_notification_events_write_identity UNIQUE (write_id, event_ordinal),
    CONSTRAINT chk_notification_events_event_ordinal CHECK (event_ordinal >= 0),
    CONSTRAINT chk_notification_events_trigger CHECK (trigger BETWEEN 1 AND 6)
) WITH (
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

CREATE TABLE notification_deliveries (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id BIGINT NOT NULL REFERENCES notification_events(id),
    subscription_key BIGINT NOT NULL REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    claimed_by TEXT COMPRESSION lz4,
    delivery_error TEXT COMPRESSION lz4,
    CONSTRAINT chk_notification_deliveries_attempt_count_non_negative
        CHECK (attempt_count >= 0),
    CONSTRAINT chk_notification_deliveries_claim_coherent
        CHECK (num_nonnulls(claimed_at, claimed_by, lease_until) IN (0, 3)),
    CONSTRAINT chk_notification_deliveries_exhausted_state CHECK (
        delivered_at IS NOT NULL
        OR attempt_count < 5
        OR num_nonnulls(claimed_at, claimed_by, lease_until) = 0),
    CONSTRAINT chk_notification_deliveries_delivery_error_bound
        CHECK (delivery_error IS NULL OR char_length(delivery_error) <= 2048)
) WITH (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_notification_deliveries_claim_candidates
    ON notification_deliveries (next_attempt_at, lease_until)
    WHERE delivered_at IS NULL AND attempt_count < 5;

CREATE INDEX idx_notification_deliveries_delivered
    ON notification_deliveries (delivered_at)
    WHERE delivered_at IS NOT NULL;

CREATE INDEX idx_notification_deliveries_subscription
    ON notification_deliveries (subscription_key);

CREATE INDEX idx_notification_deliveries_event
    ON notification_deliveries (event_id);
