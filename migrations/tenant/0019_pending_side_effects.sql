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
-- `locked_until` lease lets concurrent workers reclaim rows whose previous
-- lease expired (e.g., after a worker crash).
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
--   * `attempts` never goes below zero
--   * `locked_until` and `locked_by` are both present or both absent
--   * a dead row holds no lock (mark_dead clears the lease before dying)
--
-- Storage parameters. This relation is queue-shaped: high insert rate, every
-- row updated once or twice, every row eventually deleted. Default
-- autovacuum thresholds are proportional to table size and let that churn
-- accumulate dead tuples faster than they are reclaimed, so it gets an
-- absolute-ish threshold, a small scale factor and no vacuum cost delay.
-- `fillfactor` is set below 100 for the same reason it is set everywhere hot
-- updates happen.
-- ============================================================================

CREATE TABLE pending_side_effects (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    recorded_at TIMESTAMPTZ NOT NULL,
    write_id UUID NOT NULL,
    consequence_base INTEGER NOT NULL,
    attempts SMALLINT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_until TIMESTAMPTZ,
    locked_by TEXT,
    dead BOOLEAN NOT NULL DEFAULT FALSE,
    last_error TEXT,
    payload JSONB NOT NULL,
    CONSTRAINT chk_pse_consequence_base_non_negative CHECK (consequence_base >= 0),
    CONSTRAINT chk_pse_attempts_non_negative CHECK (attempts >= 0),
    CONSTRAINT chk_pse_lock_coherent CHECK (num_nonnulls(locked_until, locked_by) IN (0, 2)),
    CONSTRAINT chk_pse_dead_state CHECK (dead = FALSE OR num_nonnulls(locked_until, locked_by) = 0)
) WITH (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_pse_claim_candidates
    ON pending_side_effects (next_attempt_at)
    WHERE dead = FALSE;

CREATE UNIQUE INDEX idx_pse_write_identity ON pending_side_effects (write_id);
