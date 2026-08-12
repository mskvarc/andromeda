-- ============================================================================
-- 0065: Byte-wise collation for the current-state string value column
-- ============================================================================
-- Every other text column the broker compares or orders on is `COLLATE "C"`:
-- `entities.id`, `temporal_entities.id`, `attribute_names.name`,
-- `current_attribute_values.dataset_id` and `current_attribute_values.value_uri`
-- all state it. `current_attribute_values.value_string` was declared in 0057
-- without it and inherited the database's locale collation, which makes it the
-- one comparison in the query path whose result depends on the locale the
-- cluster happens to have been initialised with.
--
-- That matters in three places. Clause 4.9 orders string values with `<`, `>`,
-- `<=` and `>=`, and a locale collation orders them by language rules rather
-- than by code point, so the same `q` predicate selects different Entities on
-- two clusters holding identical data. Clause 4.5.19's `min` and `max` over a
-- string Property take the same comparison and would disagree with the temporal
-- history column in the same way. Finally, a locale-collated column plans
-- anchored `LIKE` patterns as a filter rather than as B-tree range bounds
-- unless a separate `text_pattern_ops` index exists, which is exactly the
-- second index the byte-wise declaration was chosen to avoid.
--
-- Changing a column's collation rewrites the table and rebuilds every index
-- that reads the column -- `idx_cav_attr_string` and the `idx_cav_string_trgm`
-- trigram index -- because their entries are ordered by the old collation. The
-- statement below does both, so no separate reindex is needed.
-- ============================================================================

ALTER TABLE current_attribute_values
    ALTER COLUMN value_string TYPE TEXT COLLATE "C";

-- Restated rather than assumed: the column was declared `COMPRESSION lz4` in
-- 0057 for the same reason every other payload-bearing column is, and a type
-- change is not the place to lose it silently.
ALTER TABLE current_attribute_values
    ALTER COLUMN value_string SET COMPRESSION lz4;
