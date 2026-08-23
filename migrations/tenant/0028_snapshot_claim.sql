-- ============================================================================
-- 0028: Snapshot preparation claim
-- ============================================================================
-- The on-create kick (SnapshotService::prepare_now) and the scheduled sweep
-- both look for a `preparing` snapshot to run at the same moment. Without a
-- durable claim the two workers select the same control row and both execute
-- its (expensive) queries; only the first status transition commits, so the
-- result is correct but the forwarded queries run twice.
--
-- The three claim fields form a renewable lease. A worker stamps them in the same
-- transaction that row-locks the candidate with FOR UPDATE SKIP LOCKED, so a
-- concurrent claimer skips the locked row. An expired lease can be reclaimed
-- safely by another replica.
-- ============================================================================

ALTER TABLE snapshots
    ADD COLUMN claimed_by UUID,
    ADD COLUMN claimed_at TIMESTAMPTZ,
    ADD COLUMN lease_until TIMESTAMPTZ,
    ADD CONSTRAINT chk_snapshot_claim_coherent
        CHECK (num_nonnulls(claimed_by, claimed_at, lease_until) IN (0, 3));

CREATE INDEX idx_snapshots_preparation_claim
    ON snapshots (lease_until, snapshot_priority DESC, created_at)
    WHERE status = 'preparing';
