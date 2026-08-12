-- ============================================================================
-- 0036: Give the notification outbox a real lease
-- ============================================================================
-- `claim_pending` selects undelivered due rows under FOR UPDATE SKIP LOCKED,
-- stamps `claimed_at`/`claimed_by`, commits, and delivers outside that
-- transaction. The claim predicate never excludes a live claim, so the instant
-- the claiming transaction releases its row lock a second worker can claim the
-- same still-due row and deliver it again. A claim without an expiry is not a
-- lease; `pending_side_effects` has had one since 0020 and the outbox has not.
--
-- Three things arrive together, because none of them is sufficient alone:
--   * `lease_until`, so a claim expires and a crashed worker's row is reclaimed
--     rather than stranded;
--   * a coherence constraint, so `claimed_at`, `claimed_by` and `lease_until`
--     are all present or all absent and no half-claimed state is representable;
--   * a claim-candidate index led by `next_attempt_at` and carrying
--     `lease_until`, so the lease test is answered from the index instead of a
--     heap visit per candidate -- built in 0043, which also retires the index it
--     supersedes in 0044.
--
-- Rows claimed before this migration have no lease. Filling `lease_until` from
-- `claimed_at` makes them coherent and already expired, which is the intended
-- reading of a pre-lease claim: immediately reclaimable. Any remaining partially
-- populated triple is cleared outright.
--
-- The two index statements are not here: CREATE INDEX CONCURRENTLY and DROP
-- INDEX CONCURRENTLY cannot run inside a transaction block, and the migration
-- runner sends a migration as a single multi-statement simple query, which
-- PostgreSQL executes inside an implicit one. Each therefore occupies a
-- migration of its own -- 0043 and 0044 -- whose single statement escapes that
-- implicit block. The constraint is added NOT VALID and validated in a separate
-- statement, which is the shape a deployment can split; run as one migration
-- the two share a transaction and the ADD's ACCESS EXCLUSIVE lock is held
-- across the validation scan. Every statement is individually re-runnable.
-- ============================================================================

ALTER TABLE notification_outbox ADD COLUMN IF NOT EXISTS lease_until TIMESTAMPTZ;

UPDATE notification_outbox
SET lease_until = claimed_at
WHERE claimed_at IS NOT NULL AND claimed_by IS NOT NULL AND lease_until IS NULL;

UPDATE notification_outbox
SET claimed_at = NULL, claimed_by = NULL, lease_until = NULL
WHERE num_nonnulls(claimed_at, claimed_by, lease_until) BETWEEN 1 AND 2;

ALTER TABLE notification_outbox DROP CONSTRAINT IF EXISTS chk_outbox_claim_coherent;

ALTER TABLE notification_outbox
    ADD CONSTRAINT chk_outbox_claim_coherent
    CHECK (num_nonnulls(claimed_at, claimed_by, lease_until) IN (0, 3)) NOT VALID;

ALTER TABLE notification_outbox VALIDATE CONSTRAINT chk_outbox_claim_coherent;
