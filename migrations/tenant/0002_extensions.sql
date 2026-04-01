-- ============================================================================
-- 001: PostgreSQL Extensions
-- ============================================================================
-- PostGIS:     Spatial data types, indexes (GIST), and functions (ST_Within,
--              ST_DWithin, ST_Intersects, etc.) for NGSI-LD GeoProperty
--              queries and geo-spatial filtering.
--
-- TimescaleDB: Hypertable support for the attribute_history table, enabling
--              automatic time-based partitioning (chunks), transparent
--              compression, and efficient temporal range queries required by
--              the NGSI-LD Temporal API.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS timescaledb;
