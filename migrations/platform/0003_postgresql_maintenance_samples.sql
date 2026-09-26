-- ============================================================================
-- 0003: PostgreSQL maintenance decision evidence
-- ============================================================================
-- The values behind a target's latest decision, so an operator can see why a
-- pass acted, queued or deferred work without reconstructing it from logs.
-- Every column stays NULL until the target's first sample.
--
-- Codes:
--
--   last_decision: 1 act, 2 queued, 3 would-act, 4 relation-too-small,
--     5 reclaim-too-small, 6 below-threshold, 7 consecutive-samples,
--     8 cooldown, 9 active-backend-ratio, 10 long-transaction,
--     11 active-maintenance, 12 waiting-relation-lock, 13 paired-queued
-- ============================================================================

ALTER TABLE postgresql_maintenance_targets
    ADD COLUMN last_relation_bytes BIGINT,
    ADD COLUMN last_reclaimable_bytes BIGINT,
    ADD COLUMN last_degraded_percent SMALLINT,
    ADD COLUMN last_threshold_percent SMALLINT,
    ADD COLUMN last_decision SMALLINT,
    ADD CONSTRAINT chk_postgresql_maintenance_targets_sample_non_negative
        CHECK ((last_relation_bytes IS NULL OR last_relation_bytes >= 0)
            AND (last_reclaimable_bytes IS NULL OR last_reclaimable_bytes >= 0)),
    ADD CONSTRAINT chk_postgresql_maintenance_targets_percentages
        CHECK ((last_degraded_percent IS NULL OR last_degraded_percent BETWEEN 0 AND 100)
            AND (last_threshold_percent IS NULL OR last_threshold_percent BETWEEN 0 AND 100)),
    ADD CONSTRAINT chk_postgresql_maintenance_targets_last_decision
        CHECK (last_decision IS NULL OR last_decision BETWEEN 1 AND 13);
