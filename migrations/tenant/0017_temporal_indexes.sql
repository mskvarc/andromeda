-- ============================================================================
-- 0017: Covering indexes for temporal_entity_attributes (A.8)
-- ============================================================================
-- Index-only scans for the two hottest TEA lookups:
--
--   1. "All TEAs for an entity/attribute" -- executed every time a temporal
--      write resolves a TEA id before inserting into attribute_history, and
--      every time a temporal read rebuilds the entity view. The unique
--      constraint on (entity_id, attribute_name_id, dataset_id) already
--      supports the lookup, but the heap visit to read attribute_type and
--      value_type dominates. The INCLUDE payload removes the heap visit.
--
--   2. "Distinct attribute names per entity" -- used by the temporal entity
--      renderer when the caller does not narrow with an attrs filter. The
--      existing unique constraint includes dataset_id as a trailing column,
--      so the planner falls back to a sequential scan on wide tenants. An
--      explicit (entity_id) INCLUDE (attribute_name_id) covers the scan.
--
-- Both indexes are designed to be cheap to maintain: they share the leading
-- entity_id column with the existing unique constraint, so inserts touch the
-- same btree pages.
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_tea_entity_attr
    ON temporal_entity_attributes (entity_id, attribute_name_id)
    INCLUDE (id, dataset_id, attribute_type, value_type);

CREATE INDEX IF NOT EXISTS idx_tea_entity
    ON temporal_entity_attributes (entity_id)
    INCLUDE (attribute_name_id);
