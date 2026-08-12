-- no-transaction
-- ============================================================================
-- 0044: Drop the superseded outbox pending index concurrently
-- ============================================================================
-- `idx_outbox_pending (next_attempt_at) WHERE delivered_at IS NULL AND
-- attempt_count < 5` is a strict prefix of 0043's claim-candidate index with
-- the same predicate, so it is dropped rather than left to cost a second btree
-- write per enqueued notification.
--
-- DROP INDEX CONCURRENTLY cannot run inside a transaction block and the
-- migration runner sends a migration as a single multi-statement simple query,
-- which PostgreSQL executes inside an implicit one. A migration holding exactly
-- one statement escapes that block, so this file holds exactly one.
-- ============================================================================

DROP INDEX CONCURRENTLY IF EXISTS idx_outbox_pending;
