-- ============================================================================
-- 0026: Temporal append intents
-- ============================================================================
-- Temporal history appends the broker accepted together with current Entity
-- state but has not yet applied to the temporal catalog and history. A
-- background worker leases them in `intent_key` order and applies them.
--
-- There is no `exhausted_at`. An intent is history the broker has already
-- acknowledged, so it is never discarded: an intent that keeps failing stays
-- claimable at a bounded retry cadence and is reported once its attempts pass
-- an operator threshold.
--
-- The queue exists only in the live branch. Deferral buys live ingest
-- throughput; a Snapshot branch records its history in the write's own
-- transaction, and no worker drains a branch.
-- ============================================================================

CREATE TABLE temporal_append_intents (
    intent_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- The first ordinal the intent's entries occupy in the write's numbering.
    base_ordinal INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    -- The atomic write the intent belongs to, shared by every intent of that
    -- write, so it is not the intent's identity. One write can even produce
    -- two intents for the same Entity: an inline intent for its tombstones,
    -- and one replayed later from the write's `pending_side_effects` job under
    -- the same write identity. `base_ordinal` tells them apart, hence the
    -- unique triple.
    write_id UUID NOT NULL,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    payload JSONB COMPRESSION lz4 NOT NULL,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT pk_temporal_append_intents PRIMARY KEY (intent_key),
    CONSTRAINT uq_temporal_append_intents_write_identity UNIQUE (write_id, entity_id, base_ordinal),
    CONSTRAINT chk_temporal_append_intents_attempt_count_non_negative CHECK (attempt_count >= 0),
    CONSTRAINT chk_temporal_append_intents_base_ordinal_non_negative CHECK (base_ordinal >= 0),
    CONSTRAINT chk_temporal_append_intents_claim_coherent CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_temporal_append_intents_last_error_length CHECK (last_error IS NULL OR char_length(last_error) <= 2048),
    CONSTRAINT chk_temporal_append_intents_entity_id_length CHECK (octet_length(entity_id) <= 1024)
) WITH (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim. `intent_key` is its second key because the claim orders by it,
-- so older work is never starved behind newer.
CREATE INDEX idx_temporal_append_intents_claim
    ON temporal_append_intents (next_attempt_at, intent_key);

-- The claim and the pending read scoped to one Entity, in key order.
CREATE INDEX idx_temporal_append_intents_entity
    ON temporal_append_intents (entity_id, intent_key);
