-- ============================================================================
-- 0038: Queue invariants and queue-shaped storage parameters
-- ============================================================================
-- Both durable queues express their state through columns whose combinations
-- the schema never constrained: a negative attempt counter, a half-held lock, a
-- dead or exhausted row still carrying a claim were all representable. Each
-- check below states an invariant the workers already maintain, so the database
-- refuses a bypass write or a future bug rather than trusting the writer.
--
--   pending_side_effects
--     * attempts never goes below zero
--     * `locked_until` and `locked_by` are both present or both absent
--     * a dead row holds no lock (mark_dead clears the lease before dying)
--   notification_outbox
--     * attempt_count never goes below zero
--     * an undelivered row that has exhausted its attempts holds no claim
--
-- The exhaustion bound is the same literal 5 that 0036's claim index carries.
-- One migration-owned constant governs both, and both must equal the configured
-- `outbox.max_attempts`.
--
-- Two reconciliation updates precede the constraints: pre-0036 rows can hold a
-- lock or a claim that the new invariants reject, and validating a constraint
-- against them would fail the migration rather than repair the row.
--
-- Storage parameters. Both relations are queue-shaped: high insert rate, every
-- row updated once or twice, every row eventually deleted. Default autovacuum
-- thresholds are proportional to table size and let that churn accumulate dead
-- tuples faster than they are reclaimed, so both tables get an absolute-ish
-- threshold, a small scale factor and no vacuum cost delay. `fillfactor` is set
-- below 100 for the same reason it is set everywhere hot updates happen, but it
-- is the weaker of the two levers here: claim and retry updates write
-- `next_attempt_at`, `delivered_at`, `lease_until` and `locked_until`, which
-- appear in the claim indexes and their predicates, so those updates rarely
-- qualify as heap-only and vacuum cadence is the real defence. It takes effect
-- for pages filled after this migration; existing pages keep their current fill.
--
-- Each constraint is added NOT VALID and validated separately, which is the
-- shape that lets a deployment split the two steps; run as one migration they
-- share a transaction, so the ADD's ACCESS EXCLUSIVE lock is held across the
-- validation scan. The migration runner sends a migration as a single
-- multi-statement simple query, which PostgreSQL executes inside an implicit
-- transaction block, so the statistics refresh below is ANALYZE rather than
-- VACUUM (ANALYZE); reclaiming the queues' dead tuples is what the autovacuum
-- settings above are for. Every statement is individually re-runnable:
-- constraints are dropped by name before being added, and VALIDATE on an
-- already-valid constraint does nothing.
-- ============================================================================

UPDATE pending_side_effects
SET locked_until = NULL, locked_by = NULL
WHERE dead = TRUE AND num_nonnulls(locked_until, locked_by) > 0;

UPDATE pending_side_effects
SET locked_until = NULL, locked_by = NULL
WHERE num_nonnulls(locked_until, locked_by) = 1;

UPDATE notification_outbox
SET claimed_at = NULL, claimed_by = NULL, lease_until = NULL
WHERE delivered_at IS NULL
  AND attempt_count >= 5
  AND num_nonnulls(claimed_at, claimed_by, lease_until) > 0;

ALTER TABLE pending_side_effects DROP CONSTRAINT IF EXISTS chk_pse_attempts_non_negative;

ALTER TABLE pending_side_effects
    ADD CONSTRAINT chk_pse_attempts_non_negative
    CHECK (attempts >= 0) NOT VALID;

ALTER TABLE pending_side_effects VALIDATE CONSTRAINT chk_pse_attempts_non_negative;

ALTER TABLE pending_side_effects DROP CONSTRAINT IF EXISTS chk_pse_lock_coherent;

ALTER TABLE pending_side_effects
    ADD CONSTRAINT chk_pse_lock_coherent
    CHECK (num_nonnulls(locked_until, locked_by) IN (0, 2)) NOT VALID;

ALTER TABLE pending_side_effects VALIDATE CONSTRAINT chk_pse_lock_coherent;

ALTER TABLE pending_side_effects DROP CONSTRAINT IF EXISTS chk_pse_dead_state;

ALTER TABLE pending_side_effects
    ADD CONSTRAINT chk_pse_dead_state
    CHECK (dead = FALSE OR num_nonnulls(locked_until, locked_by) = 0) NOT VALID;

ALTER TABLE pending_side_effects VALIDATE CONSTRAINT chk_pse_dead_state;

ALTER TABLE notification_outbox DROP CONSTRAINT IF EXISTS chk_outbox_attempt_count_non_negative;

ALTER TABLE notification_outbox
    ADD CONSTRAINT chk_outbox_attempt_count_non_negative
    CHECK (attempt_count >= 0) NOT VALID;

ALTER TABLE notification_outbox VALIDATE CONSTRAINT chk_outbox_attempt_count_non_negative;

ALTER TABLE notification_outbox DROP CONSTRAINT IF EXISTS chk_outbox_exhausted_state;

ALTER TABLE notification_outbox
    ADD CONSTRAINT chk_outbox_exhausted_state
    CHECK (
        delivered_at IS NOT NULL
        OR attempt_count < 5
        OR num_nonnulls(claimed_at, claimed_by, lease_until) = 0
    ) NOT VALID;

ALTER TABLE notification_outbox VALIDATE CONSTRAINT chk_outbox_exhausted_state;

ALTER TABLE pending_side_effects SET (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

ALTER TABLE notification_outbox SET (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

ANALYZE pending_side_effects;

ANALYZE notification_outbox;
