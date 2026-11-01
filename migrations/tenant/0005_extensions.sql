-- ============================================================================
-- 0005: PostgreSQL extensions
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.9, 4.10, 4.18, 4.19
--
-- The extensions the tenant schema depends on:
--
--   postgis: geometry and geography types, GiST indexes and the spatial
--     predicates behind geo-queries (clause 4.10)
--   timescaledb: hypertable partitioning of `attribute_history`
--   ltree: label paths for Scopes (clause 4.18) and their `lquery`
--     matching (clause 4.19)
--   pg_trgm: trigram GIN indexes for unanchored clause 4.9 patterns
--   btree_gist: an attribute key beside a geometry in one GiST index
--   btree_gin: an attribute key beside trigrams in one GIN index
--
-- Each is created in the `public` schema.
-- ============================================================================
create extension if not exists postgis with schema public;

create extension if not exists timescaledb with schema public;

create extension if not exists ltree with schema public;

create extension if not exists pg_trgm with schema public;

create extension if not exists btree_gist with schema public;

create extension if not exists btree_gin with schema public;
