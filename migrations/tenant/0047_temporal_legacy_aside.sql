-- ============================================================================
-- 0047: Move the legacy temporal relations aside
-- ============================================================================
-- The two history hypertables are rebuilt rather than altered: every column
-- that matters changes shape at once -- the partition column's meaning, the
-- value-number type, the instance identity, the kind columns, the four time
-- axes -- and a sequence of ALTERs on a compressed hypertable is neither
-- atomic nor cheap. This migration frees the names.
--
-- The two continuous aggregates are dropped and are not recreated. A
-- continuous aggregate must bucket on the hypertable's partition column, which
-- becomes `recorded_at` -- the operational axis that is never rendered as an
-- NGSI-LD Temporal Property. Aggregation (clause 6.3.11) buckets on the
-- requested timeproperty, defaulting to observedAt (clause 4.11), and clause
-- 5.2.21 admits only observedAt, createdAt, modifiedAt and deletedAt. The old
-- aggregates only appeared correct because the old `time` column *was* the
-- "observedAt, else write clock" fallback this rebuild removes; recreating
-- them on `recorded_at` would reinstate that fallback. Nothing in the broker
-- reads them: every aggregation compiles against the raw hypertable.
--
-- Dropping them also drops their refresh policies, which removes background
-- refresh contention from the maintenance window.
--
-- The columnstore policies are removed so no background job converts a chunk
-- while the backfill reads it. The columnstore setting itself is deliberately
-- left enabled: disabling it is refused outright on a hypertable that already
-- has columnar chunks, and removing the policy is what actually stops further
-- conversion. Existing columnar chunks stay readable and are read without
-- being converted back.
--
-- Index names are schema-global while constraint names are per-table, so only
-- the indexes have to move aside; the `chk_attribute_history_*` constraint
-- names are free for the rebuilt table to reuse. The auto-named UNIQUE
-- constraint indexes are named after their columns and cannot collide with the
-- rebuilt tables' constraint names.
-- ============================================================================

DROP MATERIALIZED VIEW IF EXISTS attr_agg_1h;
DROP MATERIALIZED VIEW IF EXISTS attr_agg_1d;

CALL remove_columnstore_policy('attribute_history', if_exists => TRUE);
CALL remove_columnstore_policy('scope_history', if_exists => TRUE);

ALTER TABLE attribute_history RENAME TO attribute_history_legacy;
ALTER TABLE scope_history RENAME TO scope_history_legacy;

ALTER INDEX idx_attribute_history_tea_time RENAME TO idx_attribute_history_legacy_tea_time;
ALTER INDEX idx_attribute_history_observed_at RENAME TO idx_attribute_history_legacy_observed_at;
ALTER INDEX idx_attribute_history_dedup RENAME TO idx_attribute_history_legacy_dedup;
ALTER INDEX idx_attribute_history_value_geo RENAME TO idx_attribute_history_legacy_value_geo;
ALTER INDEX idx_attribute_history_write RENAME TO idx_attribute_history_legacy_write;
ALTER INDEX idx_scope_history_entity_time RENAME TO idx_scope_history_legacy_entity_time;
ALTER INDEX idx_scope_history_scope_path RENAME TO idx_scope_history_legacy_scope_path;
ALTER INDEX idx_scope_history_dedup RENAME TO idx_scope_history_legacy_dedup;
