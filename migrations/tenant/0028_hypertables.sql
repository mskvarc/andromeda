-- ============================================================================
-- 0028: Temporal hypertables
-- ============================================================================
-- Converts the two live temporal history relations, attribute_history (0020)
-- and scope_history (0021), into TimescaleDB hypertables partitioned on
-- `recorded_at`. The conversion is kept out of the branch files that create the
-- tables because those files also create and upgrade every Snapshot branch,
-- where the same relations stay plain tables: a bounded, read-mostly captured
-- history gains nothing from chunking, and a branch file must be valid against
-- both shapes. This file is in `migrations/tenant/` and only ever runs in
-- `public`.
--
-- Both tables are empty when this runs, so nothing is migrated. Every index
-- the branch files created is carried onto each chunk; the one unique
-- constraint on each table leads with the partition column, which TimescaleDB
-- requires of every unique index on a hypertable. That constraint's index
-- already serves every `recorded_at`-led scan, so the default partition-column
-- index TimescaleDB would add is not created: it would duplicate a leading
-- column at the cost of one more index write per row, and it would be the one
-- index a Snapshot branch's plain table does not carry.
--
-- Chunks span seven days. The stream-led B-trees are per chunk, and only the
-- chunks still receiving rows have index pages that must stay in memory; a
-- shorter chunk keeps that active set bounded regardless of how far back the
-- table reaches. The chunk interval is a default the broker reconciles at
-- startup against its temporal configuration, so an operator change does not
-- need a migration.
--
-- Hypercore columnstore compression is not a migration's to configure: the
-- broker's storage-policy reconciler enables it at startup on the TSL edition
-- of the image, and on the Apache-2.0 core both stay plain hypertables.
-- ============================================================================

SELECT create_hypertable('attribute_history', by_range('recorded_at', INTERVAL '7 days'), create_default_indexes => false);

SELECT create_hypertable('scope_history', by_range('recorded_at', INTERVAL '7 days'), create_default_indexes => false);
