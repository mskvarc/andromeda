-- ============================================================================
-- 0024: Notification queue -- events and deliveries
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
-- Its rows are narrow and fixed-width apart from one nullable diagnostic, so a
-- claim rewrites tens of bytes instead of kilobytes.
--
-- An event with no matching Subscription is never written. The writer drops it
-- before the statement runs, so the schema never holds a payload no delivery
-- references; the reap below is for events whose deliveries have since been
-- removed, not for events that never owed one.
--
-- What the columns say, and why they are not what the outbox held:
--
--   * `write_id` and `event_ordinal` are the write identity `write_records` names,
--     carried here so a notification can be correlated with the history rows
--     and side effects of the same atomic write. `UNIQUE (write_id,
--     event_ordinal)` is what makes a replayed commit re-insert nothing rather
--     than emit a second notification for a change already announced; the
--     ordinal is the event's deterministic position within its write, so the
--     identity is stable across replays rather than dependent on insertion
--     order.
--   * `trigger` is a `SMALLINT` from the migration-owned registry the broker
--     compiles in (`andromeda-postgresql-codes`
--     `NotificationTriggerCode`), not the clause-5.2.12 token as text:
--
--       1 entityCreated, 2 entityUpdated, 3 entityDeleted,
--       4 attributeCreated, 5 attributeUpdated, 6 attributeDeleted
--
--     The check pins the range. A row carrying a number outside it could only
--     come from a release that knows a condition this schema does not, and the
--     reader refuses such a row by name rather than mapping it to a wrong
--     trigger.
--   * `changed_attribute_name_ids` are the four-byte dictionary keys of
--     `attribute_names`, not the expanded IRIs. A notification names a handful of
--     attributes out of a vocabulary the dictionary already stores once; the
--     claim resolves them back through `attribute_names` when it reads the
--     batch. The array is NOT NULL and empty for an entity-level trigger,
--     because "no attribute in particular" and "unknown" are not the same
--     statement and only one of them is true here.
--   * `entity_id` is `TEXT COLLATE "C"`, matching `entities.id`.
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
-- A delivery leaves the queue by one of two doors, and each door is its own
-- nullable instant: `delivered_at` when a subscriber acknowledged it, and
-- `exhausted_at` when the worker gave up on it -- either because the transport
-- reported a failure no retry can fix, or because the configured attempt
-- ceiling was reached. The ceiling is a delivery-policy setting the worker
-- owns; the schema does not repeat it. An earlier shape wrote the literal 5
-- into a check constraint and into the claim index predicate, which coupled a
-- runtime knob to a migration: raising the setting silently made the index
-- unusable. `exhausted_at` records the decision instead of re-deriving it from
-- `attempt_count`, so the claim predicate no longer needs to know the ceiling,
-- and the dead-letter reap has a timestamp of its own to age rows by instead
-- of borrowing the event's `created_at`. `chk_notification_deliveries_exhausted_state`
-- pins what exhaustion means: an exhausted row was never delivered and holds
-- no claim.
--
-- Index set, one named consumer each:
--
--   * `idx_notification_deliveries_claimable` is the claim's access path. The
--     claim asks one question of every row -- is it due and unheld? -- and the
--     answer is a single instant, `GREATEST(next_attempt_at,
--     COALESCE(lease_until, '-infinity'))`: the later of when it becomes due
--     and when its lease (if any) lapses. Indexing that expression, with `id`
--     as the tiebreak, makes the claim's `WHERE claimable <= now ORDER BY
--     claimable, id LIMIT n` one bounded range scan whose depth is the batch
--     size, instead of a scan led by `next_attempt_at` that visits every due
--     row to filter the leased ones out. The predicate keeps only rows still
--     in the queue, so delivered and exhausted rows fall out of the index the
--     moment they are stamped. The claim statement in
--     `andromeda-postgresql-context-store` spells the expression through the
--     one helper (`andromeda-postgresql-seaquery` `queue::claimable_at`) so it
--     matches this definition structurally.
--   * `idx_notification_deliveries_exhausted` serves the dead-letter reap's
--     range scan over exhausted rows, the same way the delivered index serves
--     the completed reap.
--   * `idx_notification_deliveries_delivered` serves the cleanup's range scan
--     over completed rows: without it the reap
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
-- gets an absolute-ish autovacuum threshold, a small scale factor, no vacuum
-- cost delay, and a fillfactor below 100. Every update a delivery row sees
-- writes a column the claim index reads -- `lease_until` and `next_attempt_at`
-- feed its expression, `delivered_at` and `exhausted_at` its predicate -- so
-- none of them is heap-only; the fillfactor is the weaker lever here, keeping
-- the new tuple version on the same page rather than relocating it, and is
-- kept modest for that reason. `notification_events` is never
-- updated and so takes no fillfactor, but it is deleted in bulk by the reap
-- and gets the same vacuum cadence for that churn.
--
-- Every relation here is created empty, so the indexes are built in this
-- migration's transaction rather than concurrently: there is nothing to lock
-- out and no scan to keep short.--
-- `claimed_by` is the schema-wide lease holder: a UUID naming one worker
-- instance, the same type on every claimable relation (`notification_deliveries`,
-- `pending_side_effects`, `temporal_append_intents`,
-- `snapshot_notification_outbox`, `snapshot_preparation`).
--
-- Branch relation: this file is in `migrations/branch/`, so it creates the live
-- relations in `public` as part of the tenant migration set and creates and
-- upgrades the same relations inside every Snapshot schema under the branch
-- ledger. Dictionary, unit-code and function references resolve to `public`
-- through the search path; every other reference resolves to the branch.
-- ============================================================================

CREATE TABLE notification_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    event_ordinal INTEGER NOT NULL,
    trigger SMALLINT NOT NULL,
    write_id UUID NOT NULL,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    changed_attribute_name_ids INTEGER[] COMPRESSION lz4 NOT NULL,
    changed_dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    entity_snapshot JSONB COMPRESSION lz4,
    previous_snapshot JSONB COMPRESSION lz4,
    CONSTRAINT uq_notification_events_write_identity UNIQUE (write_id, event_ordinal),
    CONSTRAINT chk_notification_events_event_ordinal CHECK (event_ordinal >= 0),
    CONSTRAINT chk_notification_events_trigger CHECK (trigger BETWEEN 1 AND 6)
) WITH (
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE TABLE notification_deliveries (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id BIGINT NOT NULL,
    subscription_key BIGINT NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    exhausted_at TIMESTAMPTZ,
    claimed_by UUID,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT fk_notification_deliveries_event
        FOREIGN KEY (event_id)
        REFERENCES notification_events(id) ON DELETE CASCADE,
    CONSTRAINT fk_notification_deliveries_subscription
        FOREIGN KEY (subscription_key)
        REFERENCES subscriptions(subscription_key) ON DELETE CASCADE,
    CONSTRAINT chk_notification_deliveries_attempt_count_non_negative
        CHECK (attempt_count >= 0),
    CONSTRAINT chk_notification_deliveries_claim_coherent
        CHECK (num_nonnulls(claimed_at, claimed_by, lease_until) IN (0, 3)),
    CONSTRAINT chk_notification_deliveries_exhausted_state CHECK (
        exhausted_at IS NULL
        OR (delivered_at IS NULL AND num_nonnulls(claimed_at, claimed_by, lease_until) = 0)),
    CONSTRAINT chk_notification_deliveries_last_error_bound
        CHECK (last_error IS NULL OR char_length(last_error) <= 2048)
) WITH (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_notification_deliveries_claimable
    ON notification_deliveries (GREATEST(next_attempt_at, COALESCE(lease_until, '-infinity'::timestamptz)), id)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;

CREATE INDEX idx_notification_deliveries_delivered
    ON notification_deliveries (delivered_at)
    WHERE delivered_at IS NOT NULL;

CREATE INDEX idx_notification_deliveries_exhausted
    ON notification_deliveries (exhausted_at)
    WHERE exhausted_at IS NOT NULL;

CREATE INDEX idx_notification_deliveries_subscription
    ON notification_deliveries (subscription_key);

CREATE INDEX idx_notification_deliveries_event
    ON notification_deliveries (event_id);
