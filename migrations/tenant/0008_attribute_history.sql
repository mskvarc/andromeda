-- ============================================================================
-- 007: Attribute History -- TimescaleDB Hypertable
-- ============================================================================
-- Stores the temporal evolution of entity attributes (NGSI-LD Temporal API).
--
-- Each row is a single observation/instance of an attribute at a point in
-- time. The value is stored in a *typed column* matching the attribute's
-- value_type (from temporal_entity_attributes.value_type):
--
--   value_type   ->  column
--   ----------------------------------
--   "Number"     ->  value_number   (DOUBLE PRECISION)
--   "String"     ->  value_string   (TEXT)
--   "Boolean"    ->  value_bool     (BOOLEAN)
--   "Geometry"   ->  value_geo      (PostGIS GEOMETRY)
--   "Object"     ->  value_object   (JSONB)
--   "DateTime"   ->  value_datetime (TIMESTAMPTZ)
--   "Uri"        ->  value_uri      (TEXT -- for Relationship objects)
--
-- Why typed columns instead of a single JSONB/TEXT?
--   * TimescaleDB compression is far more effective on native types
--   * Aggregate queries (AVG, SUM, MIN, MAX) work without per-row casting
--   * PostGIS spatial aggregates work directly on value_geo
--
-- Hypertable configuration:
--   * Chunk interval: 1 day -- balances query performance with chunk count
--   * Columnar storage: segmented by tea_id (each attribute stream compresses
--     independently), ordered by time DESC (recent data accessed first)
--   * Columnstore API (newer TSL): enable_columnstore + add_columnstore_policy
--   * Legacy compression (older TSL): timescaledb.compress + add_compression_policy
-- ============================================================================

CREATE TABLE attribute_history (
    -- Observation timestamp. For properties with observedAt, this is that
    -- value; otherwise it is the server-side modification time.
    time TIMESTAMPTZ NOT NULL,

    -- Reference to the attribute catalog entry
    tea_id UUID NOT NULL REFERENCES temporal_entity_attributes(id) ON DELETE CASCADE,

    -- Typed value columns -- exactly one is populated per row
    value_number DOUBLE PRECISION,
    value_string TEXT,
    value_bool BOOLEAN,
    value_geo GEOMETRY(Geometry, 4326),
    value_object JSONB,
    value_datetime TIMESTAMPTZ,
    value_uri TEXT,

    -- NGSI-LD instance identifier (clause 4.5.7) -- unique per attribute
    -- observation, used for instance-level retrieval and deletion
    instance_id TEXT NOT NULL,

    -- Original observedAt from the NGSI-LD attribute, if present
    observed_at TIMESTAMPTZ,

    -- Extra metadata: unitCode, sub-attributes, datasetId echo, etc.
    -- Stored as JSONB to accommodate the dynamic nature of NGSI-LD metadata.
    metadata JSONB,

    -- Uniqueness: one observation per attribute per timestamp per instance
    UNIQUE (tea_id, time, instance_id),

    -- Exactly one typed value column must be populated per row.
    -- The application layer determines which column based on TEA's value_type.
    CHECK (
        (CASE WHEN value_number IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_string IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_bool IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_geo IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_object IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_datetime IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN value_uri IS NOT NULL THEN 1 ELSE 0 END) = 1
    ),

    -- Geometry validity constraint for value_geo
    CONSTRAINT chk_attribute_history_value_geo_valid
        CHECK (value_geo IS NULL OR ST_IsValid(value_geo))
);

-- Convert to a TimescaleDB hypertable, partitioned by time in 1-day chunks
SELECT create_hypertable('attribute_history', 'time', chunk_time_interval => INTERVAL '1 day');

-- Enable columnar storage.
-- TimescaleDB columnstore API (enable_columnstore, add_columnstore_policy)
-- replaces legacy compression with transparent columnar storage that supports
-- DML on compressed data. When available, use columnstore; otherwise fall back
-- to legacy compression settings.
DO $$
DECLARE
    has_columnstore BOOLEAN;
BEGIN
    SELECT EXISTS(SELECT 1 FROM pg_proc WHERE proname = 'add_columnstore_policy')
    INTO has_columnstore;

    IF has_columnstore THEN
        RAISE NOTICE 'columnstore API available -- using columnstore';

        ALTER TABLE attribute_history SET (
            timescaledb.enable_columnstore,
            timescaledb.segmentby = 'tea_id',
            timescaledb.orderby = 'time DESC'
        );
        CALL add_columnstore_policy('attribute_history', after => INTERVAL '7 days');
        SET timescaledb.max_tuples_decompressed_per_dml_transaction = 1000000;
    ELSE
        RAISE NOTICE 'columnstore API not available -- using legacy compression';

        ALTER TABLE attribute_history SET (
            timescaledb.compress,
            timescaledb.compress_segmentby = 'tea_id',
            timescaledb.compress_orderby = 'time DESC'
        );
        SELECT add_compression_policy('attribute_history', INTERVAL '7 days');
    END IF;
END
$$;

-- Primary lookup: retrieve history for a specific attribute, newest first
CREATE INDEX idx_attribute_history_tea_time ON attribute_history(tea_id, time DESC);

-- Temporal queries filter by observedAt range (NGSI-LD timerel queries).
-- Without this, range queries on observed_at require sequential scans within chunks.
CREATE INDEX idx_attribute_history_observed_at ON attribute_history(tea_id, observed_at DESC)
    WHERE observed_at IS NOT NULL;

-- Dedup index: the bulk dedup DELETE uses WHERE ah.tea_id = d.tea_id AND
-- ah.instance_id = d.instance_id. The UNIQUE constraint (tea_id, time,
-- instance_id) cannot serve this lookup because `time` sits between the two
-- dedup columns. This index covers the (tea_id, instance_id) axis directly.
CREATE INDEX idx_attribute_history_dedup ON attribute_history(tea_id, instance_id);

-- Spatial index for GeoProperty values in temporal history
CREATE INDEX idx_attribute_history_value_geo ON attribute_history USING GIST (value_geo)
    WHERE value_geo IS NOT NULL;
