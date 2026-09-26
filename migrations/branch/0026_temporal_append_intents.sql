-- ============================================================================
-- 0026: Durable temporal-append intent queue
-- ============================================================================
-- `temporal_append_intents` holds ordered temporal append intents anchored
-- with current entity state, ready for a background worker to lease and
-- apply against the temporal catalogue and history hypertable.
--
-- `write_id` is the identity of the atomic commit an intent belongs to -- the
-- same identity `write_records` (0016) and `attribute_history` carry -- not the
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
-- There is no dead-letter for an intent, unlike the other durable queues: it
-- carries the shared claimable-queue lease contract (`claimed_at`,
-- `claimed_by` -- the schema-wide UUID lease holder -- `lease_until`,
-- `attempt_count`, `next_attempt_at`, `last_error`) but no `exhausted_at`
-- door. The intent is the durable record of
-- history the broker already accepted, so an intent that keeps failing is not
-- discarded: it stays claimable at a bounded retry cadence and is reported
-- once its attempt count passes the operator threshold
-- (`temporal.intent_stuck_after_attempts`). `last_error` keeps the latest
-- failure summary, cut to 2048 characters by the drain before it is written.
--
-- Physical shape. The queue is claim-shaped: a claim writes `claimed_at`,
-- `claimed_by`, `lease_until` and `attempt_count`, a retry writes
-- `next_attempt_at`, `last_error` and clears the lease, and an acknowledgement
-- deletes the row. Only `next_attempt_at` is an index key, so every claim and
-- most retries can be heap-only updates; `fillfactor = 70` leaves the page room
-- for them, and the aggressive per-table autovacuum settings reclaim the dead
-- versions a claim leaves behind before they accumulate. Column order is
-- alignment order -- not-null fixed-width descending, then nullable fixed-width,
-- then varlena -- with LZ4 on the varlena columns; `entity_id` is `COLLATE "C"`
-- like every identifier column, since it is only ever matched by equality, and
-- bounded at 1024 UTF-8 octets, the identifier bound (`IDENTIFIER_BYTE_LIMIT`
-- in `andromeda-ngsi-ld-lexicon`) an entity id is parsed under.
--
-- Claim-path note. The claim `UPDATE ... WHERE sequence IN (SELECT ... ORDER BY
-- sequence LIMIT n FOR UPDATE SKIP LOCKED)` is served by
-- `idx_temporal_append_intents_claim` on `(next_attempt_at, sequence)`. The
-- single-key `claimable_at` expression index the notification queue adopts is
-- measured and not adopted here: the claim's filter depth was measured at 0.84
-- rows removed per row returned (database-performance decision records,
-- Wave 3), under the threshold of 3 that would justify it, and `(next_attempt_at,
-- sequence)` is kept because `sequence` is the fairness key the claim orders
-- by -- older work must not be starved behind newer.
--
-- Branch relation: this file is in `migrations/branch/`, so it creates the live
-- relations in `public` as part of the tenant migration set and creates and
-- upgrades the same relations inside every Snapshot schema under the branch
-- ledger. Dictionary, unit-code and function references resolve to `public`
-- through the search path; every other reference resolves to the branch. A
-- branch file declares no foreign key into `public`: the dictionary
-- references of the live relations are added by the live-only 0029.
-- ============================================================================

CREATE TABLE temporal_append_intents (
    sequence BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    base INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    write_id UUID NOT NULL,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    intent JSONB COMPRESSION lz4 NOT NULL,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT pk_temporal_append_intents PRIMARY KEY (sequence),
    CONSTRAINT uq_temporal_append_intents_write UNIQUE (write_id, entity_id, base),
    CONSTRAINT chk_temporal_append_intents_attempt_count_non_negative CHECK (attempt_count >= 0),
    CONSTRAINT chk_temporal_append_intents_base_non_negative CHECK (base >= 0),
    CONSTRAINT chk_temporal_append_intents_claim_coherent CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_temporal_append_intents_last_error_bound CHECK (last_error IS NULL OR char_length(last_error) <= 2048),
    CONSTRAINT chk_temporal_append_intents_entity_id_length CHECK (octet_length(entity_id) <= 1024)
) WITH (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_temporal_append_intents_claim
    ON temporal_append_intents (next_attempt_at, sequence);

CREATE INDEX idx_temporal_append_intents_entity
    ON temporal_append_intents (entity_id, sequence);
