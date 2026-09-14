-- ============================================================================
-- 0019: Durable side-effect queue for opt-in async batch processing
-- ============================================================================
-- When a client requests `Prefer: respond-async` on an /entityOperations batch
-- endpoint, the broker persists the side-effect job (temporal history writes,
-- change events) into this queue in the same transaction as the entity write.
-- A background worker drains the queue with at-least-once semantics.
--
-- Crash safety: because enqueue and entity INSERT share a single transaction,
-- the only durable combinations are "both committed" or "both rolled back".
-- The worker is safe to restart: FOR UPDATE SKIP LOCKED coupled with the
-- `lease_until` lease lets concurrent workers reclaim rows whose previous
-- lease expired (e.g., after a worker crash).
--
-- The lease and dead-letter columns are the schema-wide claimable-queue
-- contract shared with the other durable queues: `claimed_at`, `claimed_by`,
-- `lease_until` are the renewable claim, `attempt_count` the attempt counter,
-- `next_attempt_at` the next-due instant, `last_error` the latest failure, and
-- `exhausted_at` the dead-letter door. Only the claim ordering and its index
-- differ between queues, on the measured grounds below.
--
-- `id` carries no default: it is minted by the broker as a version-7 UUID
-- (`andromeda-context-commit` `SideEffectJobId`), so the enqueueing code
-- knows the identity of the row before its statement runs and re-runs that
-- statement with the same value.
--
-- `write_id` carries the identity of the atomic write that produced the job.
-- One commit merges every landed write's consequences into exactly one
-- queued job, so the column is unique and the enqueue is
-- `ON CONFLICT (write_id) DO NOTHING`: a commit replayed under its original
-- identity finds its job already queued and adds no second copy, which is
-- what keeps the at-least-once drain from executing one batch's side effects
-- twice.
--
-- `recorded_at` and `consequence_base` are the commit's own `recorded_at` and
-- the first ordinal free in its consequence numbering after temporal history
-- and notification events already drew from it. A worker that leases and
-- replays this job resumes the commit's write identity and ordinal counter
-- from exactly these two values, rather than minting a new identity or
-- restarting the counter at zero, which is what keeps a crash-retried
-- delivery from duplicating the commit's consequences.
--
-- Invariants:
--   * `attempt_count` never goes below zero
--   * `claimed_at`, `lease_until` and `claimed_by` are all present or all absent
--   * an exhausted row holds no lease (dead-lettering clears the claim first)
--
-- An exhausted row is kept as evidence of a batch whose deferred side effects
-- never landed, not forever: the drain's reaper removes exhausted rows older
-- than the configured dead-letter retention
-- (`entity.pending_side_effects_dead_letter_after_secs`), and `idx_pse_exhausted`
-- is the range scan that reap runs -- partial, so live rows cost it nothing.
--
-- Storage parameters. This relation is queue-shaped: high insert rate, every
-- row updated once or twice, every row eventually deleted. Default
-- autovacuum thresholds are proportional to table size and let that churn
-- accumulate dead tuples faster than they are reclaimed, so it gets an
-- absolute-ish threshold, a small scale factor and no vacuum cost delay.
-- `fillfactor` is 50 because the claim's update -- `claimed_at`, `lease_until`,
-- `claimed_by` -- names no indexed column (the claim index is led by
-- `next_attempt_at` alone), so it is heap-only whenever the page has room,
-- and a row that carries a JSONB payload needs a full second copy's worth of
-- room for that to hold. Every varlena column is LZ4-compressed: the payload
-- is written once and read once, so decompression speed matters more than
-- ratio.
--
-- Claim-path note. The claim predicate is `exhausted_at IS NULL AND
-- next_attempt_at <= now AND (lease_until IS NULL OR lease_until <= now)`,
-- served by `idx_pse_claim_candidates` on `next_attempt_at`; leased rows are
-- filtered after the index. The single-key `GREATEST(next_attempt_at,
-- COALESCE(lease_until, '-infinity'))` expression the notification queue
-- indexes is measured and not adopted here: the claim's filter depth was
-- measured at 0.69 rows removed per row returned (database-performance
-- decision records, Wave 3), far under the threshold of 3 that would justify
-- folding `lease_until` into the index. Keeping it out is what lets the claim
-- update -- which writes `claimed_at`, `lease_until` and `claimed_by`, no
-- indexed column -- stay heap-only, so the `next_attempt_at`-led partial index
-- is kept as is.
-- ============================================================================

CREATE TABLE pending_side_effects (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    recorded_at TIMESTAMPTZ NOT NULL,
    write_id UUID NOT NULL,
    consequence_base INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    -- The shared claimable-queue lease holder. This queue's worker identifies
    -- itself with a tenant-scoped composite string (source URI plus a nonce),
    -- not a bare UUID like the notification and temporal queues, so this one
    -- holder column is TEXT; the rest of the contract is identical.
    claimed_by TEXT COMPRESSION lz4,
    exhausted_at TIMESTAMPTZ,
    last_error TEXT COMPRESSION lz4,
    payload JSONB COMPRESSION lz4 NOT NULL,
    CONSTRAINT chk_pse_consequence_base_non_negative CHECK (consequence_base >= 0),
    CONSTRAINT chk_pse_attempt_count_non_negative CHECK (attempt_count >= 0),
    CONSTRAINT chk_pse_claim_coherent CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_pse_exhausted_state CHECK (exhausted_at IS NULL OR num_nonnulls(claimed_at, lease_until, claimed_by) = 0),
    CONSTRAINT uq_pse_write_identity UNIQUE (write_id)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_pse_claim_candidates
    ON pending_side_effects (next_attempt_at)
    WHERE exhausted_at IS NULL;

CREATE INDEX idx_pse_exhausted
    ON pending_side_effects (created_at)
    WHERE exhausted_at IS NOT NULL;
