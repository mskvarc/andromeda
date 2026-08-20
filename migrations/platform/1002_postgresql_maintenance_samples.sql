-- ============================================================================
-- 1002: Auditable PostgreSQL maintenance samples
-- ============================================================================
-- Retain the values behind the latest target decision so operators can audit
-- why a pass queued or deferred work without reconstructing historical logs.
-- The fields remain nullable until a target receives its first sample.
-- ============================================================================

ALTER TABLE postgresql_maintenance_targets
    ADD COLUMN last_relation_bytes BIGINT,
    ADD COLUMN last_reclaimable_bytes BIGINT,
    ADD COLUMN last_degraded_percent SMALLINT,
    ADD COLUMN last_threshold_percent SMALLINT,
    ADD COLUMN last_decision TEXT,
    ADD CONSTRAINT chk_postgresql_maintenance_target_sample_non_negative
        CHECK ((last_relation_bytes IS NULL OR last_relation_bytes >= 0)
            AND (last_reclaimable_bytes IS NULL OR last_reclaimable_bytes >= 0)),
    ADD CONSTRAINT chk_postgresql_maintenance_target_percentages
        CHECK ((last_degraded_percent IS NULL OR last_degraded_percent BETWEEN 0 AND 100)
            AND (last_threshold_percent IS NULL OR last_threshold_percent BETWEEN 0 AND 100));
