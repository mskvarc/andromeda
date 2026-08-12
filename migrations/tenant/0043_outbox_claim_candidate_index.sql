-- no-transaction
-- ============================================================================
-- 0043: Build the outbox claim-candidate index concurrently
-- ============================================================================
-- The claim-candidate index belongs to 0036, which introduced the lease it
-- carries. It lives here instead because CREATE INDEX CONCURRENTLY cannot run
-- inside a transaction block and the migration runner sends a migration as a
-- single multi-statement simple query, which PostgreSQL executes inside an
-- implicit one. A migration holding exactly one statement escapes that block,
-- so this file holds exactly one.
--
-- The predicate repeats the attempt ceiling as the literal 5, exactly as
-- `idx_outbox_pending` did since 0013. A partial index only serves a query
-- whose predicate implies its own, so the configured `outbox.max_attempts` must
-- remain 5; raising it silently makes this index unusable rather than
-- incorrect.
--
-- If a concurrent build is interrupted it leaves an INVALID index that
-- IF NOT EXISTS will then skip forever; the operator remedy is
-- `DROP INDEX CONCURRENTLY idx_outbox_claim_candidates` before re-running, and
-- it is deliberately not automated here because doing so would drop a healthy
-- index on every ordinary re-run.
-- ============================================================================

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_outbox_claim_candidates
    ON notification_outbox (next_attempt_at, lease_until)
    WHERE delivered_at IS NULL AND attempt_count < 5;
