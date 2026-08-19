-- ============================================================================
-- 0020: Durable temporal-append intent queue and write idempotency record
-- ============================================================================
-- `temporal_append_intents` holds ordered temporal append intents anchored
-- with current entity state, ready for a background worker to lease and
-- apply against the temporal catalogue and history hypertable.
--
-- `write_id` is the identity of the atomic commit an intent belongs to -- the
-- same identity `write_records` and `attribute_history` carry -- not the
-- identity of the intent row itself: one commit lands one intent per
-- entity it touches, and every one of those intents shares the commit's
-- single write identity. That alone is not enough to identify one durable
-- intent: a batch commit in `BatchSideEffectMode::Enqueue` mode writes an
-- inline intent for an entity's tombstones under the commit's write
-- identity, and the deferred side-effect job for that same entity is later
-- replayed under the same write identity too, producing a second intent for
-- the same `(write_id, entity_id)` pair whose entries occupy a different
-- ordinal range. `base` -- the first ordinal the intent's entries occupy in
-- the commit's numbering -- is what distinguishes those two intents, so the
-- uniqueness constraint spans `(write_id, entity_id, base)` rather than
-- `(write_id, entity_id)`. `write_id` alone would additionally reject the
-- second entity of any commit that landed more than one.
--
-- `write_records` is the record of one atomic broker write, and persists
-- that write's `recorded_at` so a replay reuses the original instant instead
-- of stamping a new one. `attribute_history` is deduplicated by
-- (recorded_at, write_id, write_ordinal) -- every unique index on a
-- hypertable must contain the partition column -- so a replay that
-- re-stamped ingestion time would never collide with the rows it
-- duplicates. Reusing the persisted instant is what makes the collision
-- happen.
--
-- A `write_records` row lives for as long as any effect it protects exists,
-- and is released by Temporal Evolution deletion (clause 5.6.16) or by the
-- operator-configured horizon sweep, which is what
-- `idx_write_records_recorded_at` serves.
--
-- `accepted_at` is the instant the broker accepted the write, kept distinct
-- from `recorded_at` because a replayed write is accepted again at a later
-- moment while recording at the original one.
--
-- There is no dead-letter for an intent. The intent is the durable record of
-- history the broker already accepted, so an intent that keeps failing is not
-- discarded: it stays claimable at a bounded retry cadence and is reported
-- once its attempt count passes the operator threshold
-- (`temporal.intent_stuck_after_attempts`). `last_error` keeps the latest
-- failure summary, cut to 2048 characters by the drain before it is written.
--
-- Physical shape. The queue is claim-shaped: a claim writes `lease_owner`,
-- `lease_until` and `attempts`, a retry writes `available_at`, `last_error`
-- and clears the lease, and an acknowledgement deletes the row. Only
-- `available_at` is an index key, so every claim and most retries can be
-- heap-only updates; `fillfactor = 70` leaves the page room for them, and the
-- aggressive per-table autovacuum settings reclaim the dead versions a claim
-- leaves behind before they accumulate. Column order is alignment order --
-- not-null fixed-width descending, then nullable fixed-width, then varlena --
-- with LZ4 on the varlena columns; `entity_id` is `COLLATE "C"` like every
-- identifier column, since it is only ever matched by equality.
--
-- Claim-path note. The claim `UPDATE ... WHERE sequence IN (SELECT ... ORDER BY
-- sequence LIMIT n FOR UPDATE SKIP LOCKED)` is served by
-- `idx_temporal_append_intents_claim` on `(available_at, sequence)`. The
-- single-key `claimable_at` expression index the notification queue adopts is
-- measured and not adopted here: the claim's filter depth was measured at 0.84
-- rows removed per row returned (database-performance decision records,
-- Wave 3), under the threshold of 3 that would justify it, and `(available_at,
-- sequence)` is kept because `sequence` is the fairness key the claim orders
-- by -- older work must not be starved behind newer.
-- ============================================================================

CREATE TABLE temporal_append_intents (
    sequence BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    base INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    write_id UUID NOT NULL,
    lease_until TIMESTAMPTZ,
    lease_owner UUID,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    intent JSONB COMPRESSION lz4 NOT NULL,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT uq_temporal_append_intents_write UNIQUE (write_id, entity_id, base),
    CONSTRAINT chk_temporal_append_intents_attempts_non_negative CHECK (attempts >= 0),
    CONSTRAINT chk_temporal_append_intents_base_non_negative CHECK (base >= 0),
    CONSTRAINT chk_temporal_append_intents_lease_coherent CHECK ((lease_owner IS NULL) = (lease_until IS NULL)),
    CONSTRAINT chk_temporal_append_intents_last_error_length CHECK (last_error IS NULL OR length(last_error) <= 2048)
) WITH (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_temporal_append_intents_claim
    ON temporal_append_intents (available_at, sequence);

CREATE INDEX idx_temporal_append_intents_entity
    ON temporal_append_intents (entity_id, sequence);

CREATE TABLE write_records (
    recorded_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL,
    write_id UUID PRIMARY KEY
);

CREATE INDEX idx_write_records_recorded_at ON write_records (recorded_at);
