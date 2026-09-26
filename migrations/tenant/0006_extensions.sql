-- ============================================================================
-- 0006: PostgreSQL extensions
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.9, 4.10, 4.18, 4.19
--
-- The extensions the tenant schema depends on:
--
--   postgis: geometry and geography types, GiST indexes and the spatial
--     predicates behind geo-queries (clause 4.10)
--   timescaledb: hypertable partitioning of `attribute_history` and
--     `scope_history`
--   ltree: label paths for Scopes (clause 4.18) and their `lquery`
--     matching (clause 4.19)
--   pg_trgm: trigram GIN indexes for unanchored clause 4.9 patterns
--   btree_gist: an attribute key beside a geometry in one GiST index
--   btree_gin: an attribute key beside trigrams in one GIN index
--   pg_stat_statements: statement statistics, installed only when the server
--     preloads the library and skipped otherwise
--   pgstattuple: relation bloat diagnostics for the opt-in maintenance
--     controller, installed only when the server provides it
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS timescaledb;
CREATE EXTENSION IF NOT EXISTS ltree;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gist;
CREATE EXTENSION IF NOT EXISTS btree_gin;

DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'pg_stat_statements requires shared_preload_libraries -- skipping';
END
$$;

-- pgstattuple ships with PostgreSQL, but managed installations may omit its
-- control file; without it the maintenance controller reports a capability
-- deferral for the database. Installing it grants the broker role neither
-- execution nor `MAINTAIN`: both remain the operator's decision.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'pgstattuple') THEN
        CREATE EXTENSION IF NOT EXISTS pgstattuple;
    END IF;
END
$$;
