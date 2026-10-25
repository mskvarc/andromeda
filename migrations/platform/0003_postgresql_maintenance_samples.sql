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
--     11 active-maintenance, 12 waiting-relation-lock, 13 paired-queued,
--     14 horizon-held
-- ============================================================================
alter table postgresql_maintenance_targets
    add column last_relation_bytes bigint,
    add column last_reclaimable_bytes bigint,
    add column last_degraded_percent smallint,
    add column last_threshold_percent smallint,
    add column last_decision smallint,
    add constraint chk_postgresql_maintenance_targets_sample_non_negative check ((last_relation_bytes is null or last_relation_bytes >= 0) and (last_reclaimable_bytes is null or last_reclaimable_bytes >= 0)),
    add constraint chk_postgresql_maintenance_targets_percentages check ((last_degraded_percent is null or last_degraded_percent between 0 and 100) and (last_threshold_percent is null or last_threshold_percent between 0 and 100)),
    add constraint chk_postgresql_maintenance_targets_last_decision check (last_decision is null or last_decision between 1 and 14);
