-- ============================================================================
-- 0025: Drop redundant idx_ac_entity_attr.
-- ============================================================================
-- `idx_ac_entity_attr (entity_id, attribute_name_id)` was 1.96 GB on the live
-- broker with zero scans (`pg_stat_user_indexes.idx_scan = 0`) over a 25 M-row
-- write window. The intended consumer is the q-filter rewrite, but that
-- rewrite uses the typed-value indexes (`idx_ac_attr_num`, `_str`, etc.) which
-- already surface `entity_id` via heap fetch. The `(entity_id, attr_name_id)`
-- ordering is only useful for the inverse direction ("for entity E, list
-- attribute values"), which is served by `entities.payload` JSONB and has no
-- planned migration to `attribute_current`.
--
-- Removing it cuts ~30 % of total `attribute_current` index size and
-- eliminates one btree write per attribute observation. Reversible: a future
-- rewrite that genuinely needs the inverse direction can recreate it as part
-- of that work.
-- ============================================================================

DROP INDEX IF EXISTS idx_ac_entity_attr;
