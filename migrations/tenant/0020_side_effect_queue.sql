-- ============================================================================
-- 0020: Durable side-effect queue for opt-in async batch processing
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
-- ============================================================================

CREATE TABLE pending_side_effects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- The commit that enqueued this job: its own `recorded_at`, and the first
    -- ordinal free in its consequence numbering after temporal history and
    -- notification events already drew from it. A worker that leases and
    -- replays this job resumes the commit's write identity and ordinal
    -- counter from exactly these two values, rather than minting a new
    -- identity or restarting the counter at zero, which is what keeps a
    -- crash-retried delivery from duplicating the commit's consequences.
    recorded_at TIMESTAMPTZ NOT NULL,
    consequence_base INTEGER NOT NULL,
    attempts SMALLINT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    locked_until TIMESTAMPTZ,
    locked_by TEXT,
    dead BOOLEAN NOT NULL DEFAULT FALSE,
    last_error TEXT,
    payload JSONB NOT NULL,
    CONSTRAINT chk_pse_consequence_base_non_negative CHECK (consequence_base >= 0)
);

CREATE INDEX idx_pse_claim_candidates
    ON pending_side_effects (next_attempt_at)
    WHERE dead = FALSE;
