-- ============================================================================
-- 001: PostgreSQL Extensions
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
--   * pg_trgm            -- trigram similarity indexes for text pattern
--                           matching (available but not indexed by default)
--   * pg_stat_statements -- query performance monitoring; requires
--                           shared_preload_libraries configuration, graceful
--                           fallback if unavailable
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS timescaledb;
CREATE EXTENSION IF NOT EXISTS ltree;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'pg_stat_statements requires shared_preload_libraries -- skipping';
END
$$;
