-- ============================================================================
-- 0005: Entities -- current Entity state, keyed on a narrow surrogate identity
-- ============================================================================
-- `entity_key` is the internal identity every owned child relation carries and
-- joins on. `id` is the externally meaningful NGSI-LD URI (clause 5.6.5), stays
-- unique and stays indexed, and is what the API addresses an Entity by.
--
-- `id` is `TEXT COLLATE "C"`. Byte-wise comparison gives anchored `idPattern`
-- prefixes (clause 4.9) a plain B-tree range scan without a pattern-operator
-- index, and gives sorts abbreviated keys. `idx_entities_id_trgm` covers the
-- patterns that carry no usable anchor.
--
-- The identity declares `CACHE 32`. Entity creation is batched and concurrent,
-- and an uncached identity takes the sequence's lock once per row. Thirty-two
-- is a deliberate default rather than a measurement, and the gaps a cached
-- sequence leaves after a restart are meaningless for a surrogate that is never
-- rendered.
--
-- fillfactor and autovacuum. `entities` is rewritten on every attribute write,
-- so it is a hot-update relation by traffic. It is not one by eligibility:
-- heap-only-tuple updates are disqualified whenever a modified column appears
-- in an index key, expression or partial predicate, and both `modified_at`
-- (`idx_entities_modified_at`) and `payload` (`idx_entities_payload_root_keys`,
-- a GIN index on the expression `payload -> 0`) change on every write. The
-- fillfactor below still keeps a new tuple version on its original page more
-- often, and the autovacuum settings are the real defence.
--
-- Geometry columns are typmod-free `geometry` with explicit SRID and Zm-flag
-- checks rather than `GEOMETRY(Geometry, 4326)`: no typmod spelling accepts XY
-- and XYZ in one column, which RFC 7946 clause 3.1.1 altitudes require.
--
-- `location_geo` is a stored generated column derived from `location` via
-- `geom_to_geog`, computed by PostgreSQL and never named in an insert.
--
-- Column order is alignment order: not-null fixed-width columns first,
-- descending in width, then nullable fixed-width, then varlena, LZ4 on each
-- varlena column.
-- ============================================================================

CREATE TABLE entities (
    entity_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32) PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,
    types TEXT[] COMPRESSION lz4 NOT NULL,
    scope_paths ltree[] COMPRESSION lz4,
    payload JSONB COMPRESSION lz4 NOT NULL,
    location geometry COMPRESSION lz4,
    location_geo geography GENERATED ALWAYS AS (geom_to_geog(location)) STORED,
    observation_space geometry COMPRESSION lz4,
    operation_space geometry COMPRESSION lz4,
    CONSTRAINT chk_entities_types_present CHECK (cardinality(types) > 0),
    CONSTRAINT chk_entities_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    CONSTRAINT chk_entities_location_srid
        CHECK (location IS NULL OR ST_SRID(location) = 4326),
    CONSTRAINT chk_entities_location_zm
        CHECK (location IS NULL OR ST_Zmflag(location) IN (0, 2)),
    CONSTRAINT chk_entities_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    CONSTRAINT chk_entities_observation_space_srid
        CHECK (observation_space IS NULL OR ST_SRID(observation_space) = 4326),
    CONSTRAINT chk_entities_observation_space_zm
        CHECK (observation_space IS NULL OR ST_Zmflag(observation_space) IN (0, 2)),
    CONSTRAINT chk_entities_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space)),
    CONSTRAINT chk_entities_operation_space_srid
        CHECK (operation_space IS NULL OR ST_SRID(operation_space) = 4326),
    CONSTRAINT chk_entities_operation_space_zm
        CHECK (operation_space IS NULL OR ST_Zmflag(operation_space) IN (0, 2))
) WITH (
    fillfactor = 90,
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_entities_types ON entities USING GIN (types);

CREATE INDEX idx_entities_scope_paths ON entities USING GIST (scope_paths);

CREATE INDEX idx_entities_location ON entities USING GIST (location)
    WHERE location IS NOT NULL;

CREATE INDEX idx_entities_location_geo ON entities USING GIST (location_geo);

CREATE INDEX idx_entities_observation_space ON entities USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

CREATE INDEX idx_entities_operation_space ON entities USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

CREATE INDEX idx_entities_expires_at ON entities (expires_at)
    WHERE expires_at IS NOT NULL;

CREATE INDEX idx_entities_modified_at ON entities (modified_at);

CREATE INDEX idx_entities_id_trgm ON entities USING GIN (id gin_trgm_ops);

-- `attrs=` compiles to `payload -> 0 ? $n`; a GIN index on the bare payload
-- column cannot serve that expression.
CREATE INDEX idx_entities_payload_root_keys ON entities USING GIN ((payload -> 0));

ALTER TABLE entities ALTER COLUMN types SET STATISTICS 1000;
