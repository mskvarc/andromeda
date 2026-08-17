-- ============================================================================
-- 0020: Durable temporal-append intent queue and write idempotency record
-- ============================================================================
-- `temporal_append_intents` holds ordered temporal append intents anchored
-- with current entity state, ready for a background worker to lease and
-- apply against the temporal catalogue and history hypertable.
--
-- `intent_id` is the identity of the atomic commit an intent belongs to, not
-- the identity of the intent row itself: one commit lands one intent per
-- entity it touches, and every one of those intents shares the commit's
-- single write identity. That alone is not enough to identify one durable
-- intent: a batch commit in `BatchSideEffectMode::Enqueue` mode writes an
-- inline intent for an entity's tombstones under the commit's write
-- identity, and the deferred side-effect job for that same entity is later
-- replayed under the same write identity too, producing a second intent for
-- the same `(intent_id, entity_id)` pair whose entries occupy a different
-- ordinal range. `base` -- the first ordinal the intent's entries occupy in
-- the commit's numbering -- is what distinguishes those two intents, so the
-- uniqueness constraint spans `(intent_id, entity_id, base)` rather than
-- `(intent_id, entity_id)`. `intent_id` alone would additionally reject the
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
-- ============================================================================

CREATE TABLE temporal_append_intents (
    sequence BIGSERIAL PRIMARY KEY,
    intent_id UUID NOT NULL,
    entity_id TEXT NOT NULL,
    base INTEGER NOT NULL,
    intent JSONB NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_owner UUID,
    lease_until TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_temporal_append_intents_write UNIQUE (intent_id, entity_id, base),
    CHECK (attempts >= 0),
    CHECK (base >= 0),
    CHECK ((lease_owner IS NULL) = (lease_until IS NULL))
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
