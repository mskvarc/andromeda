-- Durable, ordered temporal append intents anchored with current entity state.
--
-- `intent_id` (renamed `write_id` by 0037) is the identity of the atomic
-- commit an intent belongs to, not the identity of the intent row itself: one
-- commit lands one intent per entity it touches, and every one of those
-- intents shares the commit's single write identity. That alone is not
-- enough to identify one durable intent: a batch commit in
-- `BatchSideEffectMode::Enqueue` mode writes an inline intent for an entity's
-- tombstones under the commit's write identity, and the deferred side-effect
-- job for that same entity is later replayed under the same write identity
-- too, producing a second intent for the same `(intent_id, entity_id)` pair
-- whose entries occupy a different ordinal range. `base` -- the first ordinal
-- the intent's entries occupy in the commit's numbering -- is what
-- distinguishes those two intents, so the uniqueness constraint spans
-- `(intent_id, entity_id, base)` rather than `(intent_id, entity_id)`.
-- `intent_id` alone would additionally reject the second entity of any commit
-- that landed more than one.

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

CREATE TABLE temporal_history_sink_receipts (
    intent_id UUID PRIMARY KEY,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE attribute_history ADD COLUMN intent_id UUID;
ALTER TABLE attribute_history ADD COLUMN intent_ordinal INTEGER;

CREATE INDEX idx_attribute_history_intent
    ON attribute_history (intent_id, intent_ordinal)
    WHERE intent_id IS NOT NULL;
