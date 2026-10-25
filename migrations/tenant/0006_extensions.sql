-- ============================================================================
-- 0006: PostgreSQL extensions
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
--   pg_stat_statements: statement statistics, installed only when the server
--     preloads the library and skipped otherwise
--   pgstattuple: relation bloat diagnostics for the opt-in maintenance
--     controller, installed only when the server provides it
-- ============================================================================
create extension if not exists postgis;

create extension if not exists timescaledb;

create extension if not exists ltree;

create extension if not exists pg_trgm;

create extension if not exists btree_gist;

create extension if not exists btree_gin;

do $$
begin
    create extension if not exists pg_stat_statements;
exception
    when others then
        raise notice 'pg_stat_statements requires shared_preload_libraries -- skipping';
end
$$;

-- pgstattuple ships with PostgreSQL, but managed installations may omit its
-- control file; without it the maintenance controller reports a capability
-- deferral for the database. Installing it grants the broker role neither
-- execution nor `MAINTAIN`: both remain the operator's decision.
do $$
begin
    if exists (
        select
            1
        from
            pg_available_extensions
        where
            name = 'pgstattuple') then
    create extension if not exists pgstattuple;
end if;
end
$$;
