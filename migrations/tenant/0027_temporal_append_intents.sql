-- ============================================================================
-- 0027: Temporal append intents
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
create table temporal_append_intents (
    intent_key bigint generated always as identity,
    created_at timestamptz not null default now(),
    next_attempt_at timestamptz not null default now(),
    -- The first ordinal the intent's entries occupy in the write's numbering.
    base_ordinal integer not null,
    attempt_count integer not null default 0,
    -- The atomic write the intent belongs to, shared by every intent of that
    -- write, so it is not the intent's identity. One write can even produce
    -- two intents for the same Entity: an inline intent for its tombstones,
    -- and one replayed later from the write's `pending_side_effects` job under
    -- the same write identity. `base_ordinal` tells them apart, hence the
    -- unique triple.
    write_id uuid not null,
    claimed_at timestamptz,
    lease_until timestamptz,
    claimed_by uuid,
    entity_id text compression lz4 collate "C" not null,
    payload jsonb compression lz4 not null,
    last_error text compression lz4,
    constraint pk_temporal_append_intents primary key (intent_key),
    constraint uq_temporal_append_intents_write_identity unique (write_id, entity_id, base_ordinal),
    constraint chk_temporal_append_intents_attempt_count_non_negative check (attempt_count >= 0),
    constraint chk_temporal_append_intents_base_ordinal_non_negative check (base_ordinal >= 0),
    constraint chk_temporal_append_intents_claim_coherent check (num_nonnulls (claimed_at, lease_until, claimed_by) in (0, 3)),
    constraint chk_temporal_append_intents_last_error_length check (last_error is null or char_length(last_error) <= 2048),
    constraint chk_temporal_append_intents_entity_id_length check (octet_length(entity_id) <= 1024)
)
with (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim. `intent_key` is its second key because the claim orders by it,
-- so older work is never starved behind newer.
create index idx_temporal_append_intents_claim on temporal_append_intents (next_attempt_at, intent_key);

-- The claim and the pending read scoped to one Entity, in key order.
create index idx_temporal_append_intents_entity on temporal_append_intents (entity_id, intent_key);

