-- ============================================================================
-- 005: Attribute History — TimescaleDB Hypertable
-- ============================================================================
-- Stores the temporal evolution of entity attributes (NGSI-LD Temporal API).
--
-- Each row is a single observation/instance of an attribute at a point in
-- time.  The value is stored in a *typed column* matching the attribute's
-- value_type (from temporal_entity_attributes.value_type):
--
--   value_type   →  column
--   ──────────────────────────
--   "Number"     →  value_number   (DOUBLE PRECISION)
--   "String"     →  value_string   (TEXT)
--   "Boolean"    →  value_bool     (BOOLEAN)
--   "Geometry"   →  value_geo      (PostGIS GEOMETRY)
--   "Object"     →  value_object   (JSONB)
--   "DateTime"   →  value_datetime (TIMESTAMPTZ)
--   "Uri"        →  value_uri      (TEXT — for Relationship objects)
--
-- Why typed columns instead of a single JSONB/TEXT?
--   • TimescaleDB compression is far more effective on native types
--   • Aggregate queries (AVG, SUM, MIN, MAX) work without per-row casting
--   • PostGIS spatial aggregates work directly on value_geo
--
-- Hypertable configuration:
--   • Chunk interval: 1 day — balances query performance with chunk count
--   • Compression: segmented by tea_id (each attribute stream compresses
--     independently), ordered by time DESC (recent data accessed first)
--   • Compression policy: data older than 7 days is automatically compressed
-- ============================================================================

CREATE TABLE attribute_history (
    -- The observation timestamp. For properties with observedAt, this is
    -- that value; otherwise it's the server-side modification time.
    time TIMESTAMPTZ NOT NULL,

    -- Reference to the attribute catalog entry
    tea_id UUID NOT NULL REFERENCES temporal_entity_attributes(id) ON DELETE CASCADE,

    -- Typed value columns — exactly one is populated per row
    value_number DOUBLE PRECISION,
    value_string TEXT,
    value_bool BOOLEAN,
    value_geo GEOMETRY(Geometry, 4326),
    value_object JSONB,
    value_datetime TIMESTAMPTZ,
    value_uri TEXT,

    -- NGSI-LD instance identifier (clause 4.5.7) — unique per attribute
    -- observation, used for instance-level retrieval and deletion.
    instance_id TEXT NOT NULL,

    -- The original observedAt from the NGSI-LD attribute, if present
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
    )
);

-- Convert to a TimescaleDB hypertable, partitioned by time in 1-day chunks.
SELECT create_hypertable('attribute_history', 'time', chunk_time_interval => INTERVAL '1 day');

-- Enable native columnar compression.
-- Segmenting by tea_id means each attribute's time-series compresses as an
-- independent stream, yielding excellent compression ratios.
ALTER TABLE attribute_history SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'tea_id',
    timescaledb.compress_orderby = 'time DESC'
);

-- Automatically compress chunks older than 7 days in the background.
SELECT add_compression_policy('attribute_history', INTERVAL '7 days');

-- Primary lookup pattern: retrieve history for a specific attribute, newest first.
CREATE INDEX idx_attribute_history_tea_time ON attribute_history(tea_id, time DESC);

-- Temporal queries commonly filter by observedAt range (NGSI-LD timerel queries).
-- Without this, range queries on observed_at would require sequential scans within chunks.
CREATE INDEX idx_attribute_history_observed_at ON attribute_history(tea_id, observed_at DESC)
    WHERE observed_at IS NOT NULL;
