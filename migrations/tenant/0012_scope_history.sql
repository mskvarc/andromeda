-- ============================================================================
-- 012: Scope History -- Dedicated TimescaleDB Hypertable
-- ============================================================================
-- Stores the temporal evolution of entity scopes (NGSI-LD clause 4.5.4).
--
-- Scopes are NOT attributes -- they have no dataset_id, no value_type
-- taxonomy, and no TEA catalog entry. Previous versions piggybacked scope
-- changes as synthetic Property instances in attribute_history; this table
-- gives scopes their own dedicated storage with proper ltree indexing.
--
-- The scope path IS the value, stored as PostgreSQL ltree for efficient
-- hierarchical matching (e.g. scopeQ with wildcards).
--
-- Hypertable configuration mirrors attribute_history (0008):
--   * Chunk interval: 1 day
--   * Columnar storage: segmented by entity_id, ordered by time DESC
-- ============================================================================

CREATE TABLE scope_history (
    -- Observation timestamp (server-side modification time).
    time          TIMESTAMPTZ NOT NULL,

    -- The entity this scope belongs to.
    entity_id     TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,

    -- The scope path in ltree format (e.g. 'Madrid.Gardens').
    -- Slash-delimited NGSI-LD scopes are converted to dot notation on insert.
    scope_path    ltree NOT NULL,

    -- Unique identifier for this scope observation instance.
    instance_id   TEXT NOT NULL,

    -- Uniqueness: one scope observation per entity per timestamp per instance.
    UNIQUE (entity_id, time, instance_id)
);

-- Convert to a TimescaleDB hypertable, partitioned by time in 1-day chunks.
SELECT create_hypertable('scope_history', 'time', chunk_time_interval => INTERVAL '1 day');

-- Enable columnar storage.
-- Same pattern as attribute_history (0008): prefer columnstore API, fall back
-- to legacy compression on older TimescaleDB versions.
DO $$
DECLARE
    has_columnstore BOOLEAN;
BEGIN
    SELECT EXISTS(SELECT 1 FROM pg_proc WHERE proname = 'add_columnstore_policy')
    INTO has_columnstore;

    IF has_columnstore THEN
        RAISE NOTICE 'columnstore API available -- using columnstore for scope_history';

        ALTER TABLE scope_history SET (
            timescaledb.enable_columnstore,
            timescaledb.segmentby = 'entity_id',
            timescaledb.orderby = 'time DESC'
        );
        CALL add_columnstore_policy('scope_history', after => INTERVAL '7 days');
        SET timescaledb.max_tuples_decompressed_per_dml_transaction = 1000000;
    ELSE
        RAISE NOTICE 'columnstore API not available -- using legacy compression for scope_history';

        ALTER TABLE scope_history SET (
            timescaledb.compress,
            timescaledb.compress_segmentby = 'entity_id',
            timescaledb.compress_orderby = 'time DESC'
        );
        SELECT add_compression_policy('scope_history', INTERVAL '7 days');
    END IF;
END
$$;

-- Primary lookup: retrieve scope history for an entity, newest first.
CREATE INDEX idx_scope_history_entity_time ON scope_history(entity_id, time DESC);

-- Hierarchical scope matching for scopeQ queries using lquery/ltree operators.
CREATE INDEX idx_scope_history_scope_path ON scope_history USING GIST (scope_path);

-- Dedup index: the bulk dedup DELETE uses WHERE entity_id = d.entity_id AND
-- instance_id = d.instance_id. The UNIQUE constraint includes `time` between
-- the dedup columns, so this index covers the (entity_id, instance_id) axis.
CREATE INDEX idx_scope_history_dedup ON scope_history(entity_id, instance_id);

-- ============================================================================
-- Data migration: move synthetic scope entries from attribute_history
-- ============================================================================
-- Previous versions recorded scopes as fake Property instances in
-- attribute_history with attribute name 'https://uri.etsi.org/ngsi-ld/scope'.
-- Migrate those rows to scope_history, converting slash paths to ltree dots.

INSERT INTO scope_history (time, entity_id, scope_path, instance_id)
SELECT ah.time, tea.entity_id,
       REPLACE(LTRIM(ah.value_string, '/'), '/', '.')::ltree,
       ah.instance_id
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
JOIN attribute_names an ON an.id = tea.attribute_name_id
WHERE an.name = 'https://uri.etsi.org/ngsi-ld/scope';

-- Clean up synthetic scope entries from attribute_history.
DELETE FROM attribute_history ah
USING temporal_entity_attributes tea, attribute_names an
WHERE ah.tea_id = tea.id AND an.id = tea.attribute_name_id
AND an.name = 'https://uri.etsi.org/ngsi-ld/scope';

-- Clean up synthetic scope TEA catalog entries.
DELETE FROM temporal_entity_attributes tea
USING attribute_names an
WHERE an.id = tea.attribute_name_id
AND an.name = 'https://uri.etsi.org/ngsi-ld/scope';
