-- ============================================================================
-- 0022: Current attribute values
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.6.3, 4.9, 4.10, 4.23, 5.6.6
--
-- The value each attribute instance of each Entity currently has, one row per
-- Entity, attribute name and `datasetId` (clause 4.5.5), in one typed column
-- per value shape so a `q` comparison reads a single indexed column. The only
-- ownership edge is to `entities`: Core Entity deletion (clause 5.6.6) leaves
-- history standing, so nothing here references the temporal catalog.
--
-- Codes:
--
--   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--     4 VocabProperty, 5 LanguageProperty, 6 JsonProperty, 7 ListProperty,
--     8 ListRelationship
--   value_kind: 1 number, 2 string, 3 boolean, 4 geometry, 5 object,
--     6 date-time, 7 uri
--
-- Value kind 0, the tombstone, never appears here: a deleted attribute is an
-- absent row, not a row carrying a marker.
--
-- Clause 4.6.3 names three data types, DateTime, Date and Time, whose whole
-- representation is the string, so a value written in one of their lexical
-- forms is projected as that data type. A DateTime lands in
-- `value_date_time`. A Date or a Time lands in `value_string` in its
-- canonical fixed-width form (`YYYY-MM-DD`, `HH:MM:SS.ffffffZ`), so byte order
-- is chronological order and a clause 4.9 comparison orders it correctly.
--
-- There is no `modified_at`: nothing reads when a projected value was last
-- written, since `entity_documents.modified_at` carries the Entity's instant.
-- A per-row instant would turn every restatement of an unchanged value into a
-- real row rewrite, which is the write the upsert's no-op guard avoids: an
-- instance whose projection is value-identical is not rewritten at all. The
-- guard compares with each column type's own equality, under which `NUMERIC`
-- ignores scale (`1.0 = 1.00`); no reader renders a projected number, it only
-- compares it.
-- ============================================================================

CREATE TABLE current_attribute_values (
    entity_key BIGINT NOT NULL,
    -- `attribute_names` key.
    attribute_name_key INTEGER NOT NULL,
    attribute_kind SMALLINT NOT NULL,
    value_kind SMALLINT NOT NULL,
    value_date_time TIMESTAMPTZ,
    unit_code_id SMALLINT,
    value_boolean BOOLEAN,
    dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    -- `NUMERIC`, not `DOUBLE PRECISION`: JSON numbers are stored exactly, and
    -- a comparison beyond 2^53 stays correct without falling back to the
    -- payload.
    value_number NUMERIC COMPRESSION lz4,
    value_string TEXT COMPRESSION lz4 COLLATE "C",
    value_geometry geometry COMPRESSION lz4,
    value_object JSONB COMPRESSION lz4,
    -- A Relationship object, an identifier, so it never needs an overflow
    -- column.
    value_uri TEXT COMPRESSION lz4 COLLATE "C",
    -- A String too long for the B-tree on `value_string`. The two length
    -- checks give every String exactly one column, so an equality reads only
    -- the column its operand would occupy.
    value_long_text TEXT COMPRESSION lz4 COLLATE "C",
    CONSTRAINT fk_current_attribute_values_entity
        FOREIGN KEY (entity_key)
        REFERENCES entities(entity_key) ON DELETE CASCADE,
    -- The grain has a nullable `dataset_id`, so it is a unique constraint and
    -- the table has no primary key. It is the upsert's conflict target and the
    -- access path of every Entity-correlated `q` probe.
    CONSTRAINT uq_current_attribute_values_grain
        UNIQUE NULLS NOT DISTINCT (entity_key, attribute_name_key, dataset_id),
    CONSTRAINT chk_current_attribute_values_attribute_kind
        CHECK (attribute_kind BETWEEN 1 AND 8),
    CONSTRAINT chk_current_attribute_values_value_presence CHECK (
        num_nonnulls(value_number, value_string, value_boolean, value_geometry,
                     value_object, value_date_time, value_uri, value_long_text) = 1),
    CONSTRAINT chk_current_attribute_values_value_kind_column CHECK (
        (value_kind = 1 AND value_number IS NOT NULL)
        OR (value_kind = 2 AND num_nonnulls(value_string, value_long_text) = 1)
        OR (value_kind = 3 AND value_boolean IS NOT NULL)
        OR (value_kind = 4 AND value_geometry IS NOT NULL)
        OR (value_kind = 5 AND value_object IS NOT NULL)
        OR (value_kind = 6 AND value_date_time IS NOT NULL)
        OR (value_kind = 7 AND value_uri IS NOT NULL)),
    CONSTRAINT chk_current_attribute_values_value_geometry_valid
        CHECK (value_geometry IS NULL OR ST_IsValid(value_geometry)),
    CONSTRAINT chk_current_attribute_values_value_geometry_srid
        CHECK (value_geometry IS NULL OR ST_SRID(value_geometry) = 4326),
    CONSTRAINT chk_current_attribute_values_value_geometry_zm
        CHECK (value_geometry IS NULL OR ST_Zmflag(value_geometry) IN (0, 2)),
    CONSTRAINT chk_current_attribute_values_value_string_length
        CHECK (octet_length(value_string) <= 2048),
    CONSTRAINT chk_current_attribute_values_value_long_text_length
        CHECK (octet_length(value_long_text) > 2048),
    CONSTRAINT chk_current_attribute_values_value_uri_length
        CHECK (octet_length(value_uri) <= 1024),
    CONSTRAINT chk_current_attribute_values_dataset_id_length
        CHECK (octet_length(dataset_id) <= 1024)
) WITH (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- One partial B-tree per typed column, led by the attribute key, for plans
-- that drive from the attribute side.
CREATE INDEX idx_cav_value_number
    ON current_attribute_values (attribute_name_key, value_number)
    WHERE value_number IS NOT NULL;

CREATE INDEX idx_cav_value_string
    ON current_attribute_values (attribute_name_key, value_string)
    WHERE value_string IS NOT NULL;

-- Unanchored `~=` and `!~=` patterns (clause 4.9). `fastupdate = off`: the
-- projection is written inside the Entity's transaction, and a GIN pending
-- list would put its flush latency on the request path.
CREATE INDEX idx_cav_value_string_trgm
    ON current_attribute_values USING GIN (attribute_name_key int4_ops, value_string gin_trgm_ops)
    WITH (fastupdate = off)
    WHERE value_string IS NOT NULL;

CREATE INDEX idx_cav_value_boolean
    ON current_attribute_values (attribute_name_key, value_boolean)
    WHERE value_boolean IS NOT NULL;

CREATE INDEX idx_cav_value_date_time
    ON current_attribute_values (attribute_name_key, value_date_time)
    WHERE value_date_time IS NOT NULL;

CREATE INDEX idx_cav_value_uri
    ON current_attribute_values (attribute_name_key, value_uri)
    WHERE value_uri IS NOT NULL;

-- A long String is too long to key a B-tree on, so a `q` comparison over it
-- drives from the attribute key alone and filters the value on the heap.
CREATE INDEX idx_cav_value_long_text
    ON current_attribute_values (attribute_name_key)
    WHERE value_long_text IS NOT NULL;

-- Patterns over long Strings, and their equality, which `gin_trgm_ops` also
-- serves. Neither long-text index is a maintenance target: they cover rare
-- values, and vacuuming the table reclaims their dead entries.
CREATE INDEX idx_cav_value_long_text_trgm
    ON current_attribute_values USING GIN (attribute_name_key int4_ops, value_long_text gin_trgm_ops)
    WITH (fastupdate = off)
    WHERE value_long_text IS NOT NULL;

-- Geo-queries against a named GeoProperty (clause 4.10).
CREATE INDEX idx_cav_value_geometry
    ON current_attribute_values USING GIST (attribute_name_key, value_geometry)
    WHERE value_geometry IS NOT NULL;

-- The correlated instance lookup and the distance ordering of clause 4.23.
CREATE INDEX idx_cav_entity_geometry
    ON current_attribute_values (attribute_name_key, entity_key)
    WHERE value_geometry IS NOT NULL;

CREATE STATISTICS stx_cav_attribute_value_kind (dependencies, ndistinct)
    ON attribute_name_key, value_kind
    FROM current_attribute_values;

CREATE STATISTICS stx_cav_entity_attribute (dependencies, ndistinct)
    ON entity_key, attribute_name_key
    FROM current_attribute_values;
