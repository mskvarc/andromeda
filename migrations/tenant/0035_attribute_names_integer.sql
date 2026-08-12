-- ============================================================================
-- 0035: Widen the attribute-name dictionary key from SMALLINT to INTEGER
-- ============================================================================
-- `attribute_names.id` is a positive SMALLINT identity, so the dictionary can
-- hand out at most 32,767 keys. Attribute IRIs are open-world domain values and
-- NGSI-LD sets no corresponding limit, so that ceiling is a capacity accident
-- rather than a modelled constraint. Every column that carries the key is
-- widened in this one migration: the dictionary identity, its owning sequence,
-- and the two referencing columns.
--
-- Referencing columns, from 0007 and 0015:
--   temporal_entity_attributes.attribute_name_id
--   attribute_current.attribute_name_id
-- `attribute_history` carries no dictionary key -- it references the catalogue
-- through `tea_id` -- so no hypertable is touched here.
--
-- SMALLINT and INTEGER differ in width, so each ALTER rewrites its table under
-- ACCESS EXCLUSIVE and rebuilds every index on it, including the btree_gist
-- composite `idx_ac_attr_geo` and the partial typed-value indexes. On a large
-- tenant this is maintenance-window work, and `attribute_current` is the
-- expensive one. The parent is widened first; while the children still hold
-- SMALLINT the foreign keys compare int2 to int4, which PostgreSQL accepts.
--
-- `ALTER SEQUENCE ... AS integer` raises the sequence maximum from the SMALLINT
-- bound to the INTEGER bound. It is a no-op where the column retype already
-- propagated to the identity sequence, and it is stated explicitly so the
-- ceiling does not survive the column change on any server version.
--
-- A rewrite invalidates the planner statistics of every rewritten table, so the
-- migration ends with ANALYZE. Freezing the rewritten heaps is left to
-- autovacuum: the migration runner sends a migration as a single multi-statement
-- simple query, which PostgreSQL executes inside an implicit transaction block,
-- and VACUUM cannot run there. Every statement below is individually
-- re-runnable, since retyping a column to the type it already has is a no-op.
--
-- The widened dictionary stays append-only and therefore open to inflation: any
-- client can mint unbounded distinct attribute IRIs. INTEGER buys headroom, not
-- protection. Rate limiting new-name creation and observing dictionary growth
-- are API-layer concerns and are deliberately absent from this migration.
-- ============================================================================

ALTER TABLE attribute_names ALTER COLUMN id TYPE INTEGER;

ALTER SEQUENCE attribute_names_id_seq AS INTEGER;

ALTER TABLE temporal_entity_attributes ALTER COLUMN attribute_name_id TYPE INTEGER;

ALTER TABLE attribute_current ALTER COLUMN attribute_name_id TYPE INTEGER;

ANALYZE attribute_names;

ANALYZE temporal_entity_attributes;

ANALYZE attribute_current;
