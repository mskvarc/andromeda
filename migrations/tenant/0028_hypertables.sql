-- ============================================================================
-- 0028: Temporal hypertables
-- ============================================================================
-- Converts the live `attribute_history` and `scope_history` into TimescaleDB
-- hypertables partitioned on `recorded_at`. A Snapshot schema keeps both as
-- plain tables: a bounded, read-mostly captured history gains nothing from
-- chunking. Both tables are still empty when they are converted, so no row
-- moves, and every index they already carry is created on each chunk.
--
-- The default partition-column index is not created. TimescaleDB requires
-- every unique index on a hypertable to contain the partition column, so each
-- table's one unique constraint already leads with `recorded_at` and serves
-- every `recorded_at`-led scan. The default index would duplicate it at the
-- cost of one more index write per row.
--
-- Chunks initially span seven days. The stream-led B-trees are per chunk, and
-- only chunks still receiving rows keep their index pages hot, so a shorter
-- chunk bounds that working set however far back the history reaches.
-- ============================================================================

SELECT create_hypertable('attribute_history', by_range('recorded_at', INTERVAL '7 days'), create_default_indexes => false);

SELECT create_hypertable('scope_history', by_range('recorded_at', INTERVAL '7 days'), create_default_indexes => false);
