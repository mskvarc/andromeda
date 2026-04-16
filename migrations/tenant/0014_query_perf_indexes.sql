-- ============================================================================
-- 014: Query Performance Indexes
-- ============================================================================
-- Non-breaking additions that improve planner coverage on the hot query paths.
--
-- * pg_trgm: enables trigram GIN index on entities.id for idPattern regex
--            searches that currently fall back to sequential scans.
-- * idx_entities_payload_pathops: JSONB containment (@>) index that the
--   planner can pick when q filters are rewritten to use containment. Lives
--   alongside the default jsonb_ops index so both operator classes remain
--   available.
-- * Statistics target on entities.types: GIN multi-array scans benefit from
--   per-element cardinality statistics; the default target of 100 is too low
--   for selective type predicates on large datasets.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX idx_entities_id_trgm
    ON entities USING GIN (id gin_trgm_ops);

CREATE INDEX idx_entities_payload_pathops
    ON entities USING GIN (payload jsonb_path_ops);

ALTER TABLE entities ALTER COLUMN types SET STATISTICS 1000;
ANALYZE entities;
