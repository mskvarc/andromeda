-- ============================================================================
-- 0028: Snapshot preparation claim
-- ============================================================================
-- The on-create kick (SnapshotService::prepare_now) and the scheduled sweep
-- both look for a `preparing` snapshot to run at the same moment. Without a
-- durable claim the two workers select the same control row and both execute
-- its (expensive) queries; only the first status transition commits, so the
-- result is correct but the forwarded queries run twice.
--
-- `claimed_at` is that durable claim. A worker stamps it in the same
-- transaction that row-locks the candidate with FOR UPDATE SKIP LOCKED, so a
-- concurrent claimer skips the locked row and the `claimed_at IS NULL` guard
-- keeps it skipped once committed: exactly one worker ever prepares a given
-- snapshot. A claim orphaned by a worker that dies mid-preparation is reaped
-- with the snapshot itself at `expires_at`.
-- ============================================================================

ALTER TABLE snapshots ADD COLUMN claimed_at TIMESTAMPTZ;
