-- ============================================================================
-- 0025: Pending side effects
-- ============================================================================
-- When a batch entity operation asks for `Prefer: respond-async`, the broker
-- queues the write's side effects (temporal history, change events) here in
-- the same transaction as the Entity write, so either both commit or neither
-- does. A background worker drains the queue at least once per job, and a
-- worker that crashes leaves its lease to lapse for another to reclaim.
--
-- The queue exists only in the live branch. A request scoped to a Snapshot
-- runs its side effects inline, so no Snapshot branch ever has a job to
-- queue, and no worker would drain one.
-- ============================================================================

CREATE TABLE pending_side_effects (
    -- Minted by the broker before the statement runs, so a retried statement
    -- carries the same identity.
    id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- With `base_ordinal`, what a replay resumes from: the commit's recorded
    -- instant, and the first ordinal left free after its history and events
    -- drew from the commit's numbering. A crash-retried job reuses both
    -- rather than minting a new identity or restarting at zero, so it never
    -- duplicates the commit's consequences.
    recorded_at TIMESTAMPTZ NOT NULL,
    -- One commit queues exactly one job, so the enqueue does nothing on a
    -- conflict here: a commit replayed under its original identity finds its
    -- job already queued, and the drain never runs one batch's side effects
    -- twice.
    write_id UUID NOT NULL,
    base_ordinal INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    exhausted_at TIMESTAMPTZ,
    last_error TEXT COMPRESSION lz4,
    payload JSONB COMPRESSION lz4 NOT NULL,
    CONSTRAINT pk_pending_side_effects PRIMARY KEY (id),
    CONSTRAINT chk_pse_base_ordinal_non_negative CHECK (base_ordinal >= 0),
    CONSTRAINT chk_pse_attempt_count_non_negative CHECK (attempt_count >= 0),
    CONSTRAINT chk_pse_claim_coherent CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_pse_exhausted_state CHECK (exhausted_at IS NULL OR num_nonnulls(claimed_at, lease_until, claimed_by) = 0),
    CONSTRAINT chk_pse_last_error_length CHECK (last_error IS NULL OR char_length(last_error) <= 2048),
    CONSTRAINT uq_pse_write_identity UNIQUE (write_id)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim. Leased rows are filtered after the scan, which keeps the lease
-- columns out of every index, so a claim stays a heap-only update.
CREATE INDEX idx_pse_claim
    ON pending_side_effects (next_attempt_at)
    WHERE exhausted_at IS NULL;

-- The reap of dead-lettered jobs.
CREATE INDEX idx_pse_exhausted_at
    ON pending_side_effects (exhausted_at)
    WHERE exhausted_at IS NOT NULL;
