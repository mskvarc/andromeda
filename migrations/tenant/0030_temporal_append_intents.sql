-- Durable, ordered temporal append intents anchored with current entity state.

CREATE TABLE temporal_append_intents (
    sequence BIGSERIAL PRIMARY KEY,
    intent_id UUID NOT NULL UNIQUE,
    entity_id TEXT NOT NULL,
    intent JSONB NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_owner UUID,
    lease_until TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (attempts >= 0),
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
