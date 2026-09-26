-- ============================================================================
-- 0006: PostgreSQL Extensions
-- ============================================================================
-- Required extensions for spatial, temporal, hierarchical, and diagnostic
-- functionality.
--
--   * postgis            -- spatial data types, GIST indexes, and functions
--                           (ST_Within, ST_DWithin, ST_Intersects, etc.) for
--                           NGSI-LD GeoProperty queries and geo-spatial filtering
--   * timescaledb        -- hypertable support for attribute_history, enabling
--                           automatic time-based partitioning, transparent
--                           compression, and efficient temporal range queries
--                           required by the NGSI-LD Temporal API
--   * ltree              -- hierarchical label tree type for NGSI-LD Scopes
--                           (clause 4.18), supporting lquery/ltxtquery operators
--                           for scope path matching
--   * pg_trgm            -- trigram GIN indexes for unanchored clause 4.9
--                           patterns: `idx_entities_id_trgm` and
--                           `idx_temporal_entities_id_trgm` serve an
--                           `idPattern`, `idx_cav_attr_string_trgm` and
--                           `idx_cav_attr_long_text_trgm` a `~=` over string
--                           values
--   * btree_gist         -- enables GIST indexes on scalar types (e.g. SMALLINT)
--                           alongside geometry columns, used for composite
--                           (attribute_name_id, value_geo) indexes
--   * btree_gin          -- enables GIN entries on scalar types, used for the
--                           attribute-aware trigram index
--                           (attribute_name_id, value_string) on the current
--                           attribute-value projection
--   * pg_stat_statements -- query performance monitoring; requires
--                           shared_preload_libraries configuration, graceful
--                           fallback if unavailable
--   * pgstattuple        -- physical relation diagnostics (bloat and free
--                           space) for the opt-in maintenance controller; a
--                           supplied module managed installs may omit, so it is
--                           installed only when its control file is present
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

-- pgstattuple is a PostgreSQL supplied module, but managed installations may
-- omit its control file. Install it when available and let the opt-in
-- maintenance controller report a per-database capability deferral otherwise.
-- This never grants execution or MAINTAIN privileges to the broker role; those
-- remain explicit operator decisions.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'pgstattuple') THEN
        CREATE EXTENSION IF NOT EXISTS pgstattuple;
    END IF;
END
$$;
