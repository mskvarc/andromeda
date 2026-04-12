-- ============================================================================
-- 004: Entity Registry
-- ============================================================================
-- Holds the *current state* of every NGSI-LD Entity.
--
-- The full expanded JSON-LD document lives in `payload` (JSONB), which
-- preserves the dynamic, schema-less nature of NGSI-LD. Critical search
-- fields are "hoisted" into top-level SQL columns so they can be indexed
-- natively -- avoiding slow JSON parsing during filtering.
--
-- Hoisted columns:
--   * types[]           -- GIN-indexed array for fast type-based filtering
--   * scope_paths[]     -- GIST-indexed ltree array for hierarchical scope
--                          matching (clause 4.18)
--   * location          -- PostGIS geometry extracted from the entity's
--                          default GeoProperty named "location", enabling
--                          GIST-indexed spatial queries (within, near, etc.)
--   * location_geo      -- native geography column derived from location via
--                          geom_to_geog(), avoiding runtime geometry->geography
--                          casts for distance queries
--   * observation_space -- ETSI GS CIM 009 clause 5.2.4, Table 5.2.4-1
--   * operation_space   -- ETSI GS CIM 009 clause 5.2.4, Table 5.2.4-1
--   * payload           -- GIN-indexed JSONB for ad-hoc JSONPath queries (q filter)
-- ============================================================================

CREATE TABLE entities (
    -- URI identifier, e.g. "urn:ngsi-ld:Vehicle:A1"
    id TEXT NOT NULL PRIMARY KEY,

    -- Entity type(s), e.g. '{"Vehicle", "WasteManagement"}'.
    -- An entity may belong to multiple types per NGSI-LD spec.
    types TEXT[] NOT NULL,

    -- NGSI-LD Scopes (clause 4.18) as ltree paths for hierarchical matching,
    -- e.g. '{"Madrid.Gardens", "Madrid.Traffic"}'
    scope_paths ltree[],

    -- Materialized geometry from the entity's "location" GeoProperty.
    -- Supports Point, Polygon, LineString, etc. in WGS 84 (SRID 4326).
    location GEOMETRY(Geometry, 4326),

    -- Native geography column for distance-based queries (ST_DWithin on
    -- geography uses metres, avoids runtime geometry->geography cast).
    -- Derived from location via the geom_to_geog() immutable wrapper.
    location_geo GEOGRAPHY(Geometry, 4326)
        GENERATED ALWAYS AS (geom_to_geog(location)) STORED,

    -- Observation space GeoProperty (clause 5.2.4)
    observation_space GEOMETRY(Geometry, 4326),

    -- Operation space GeoProperty (clause 5.2.4)
    operation_space GEOMETRY(Geometry, 4326),

    -- Full expanded JSON-LD document -- the source of truth for all
    -- attribute data. Rust maps this via sqlx::types::Json<Value>.
    payload JSONB NOT NULL,

    -- Transient storage support (NGSI-LD clause 4.22).
    -- When set, the entity becomes invalid after this timestamp.
    expires_at TIMESTAMPTZ,

    -- System-managed temporal properties (sysAttrs)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Geometry validity constraints (defensive layer against application
    -- bugs or direct SQL modifications; ST_MakeValid() is applied at ingestion)
    CONSTRAINT chk_entities_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    CONSTRAINT chk_entities_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    CONSTRAINT chk_entities_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space))
);

-- Fast filtering by entity type(s): WHERE 'Vehicle' = ANY(types)
CREATE INDEX idx_entities_types ON entities USING GIN (types);

-- Fast hierarchical scope matching: scope_paths ~ 'A.B.*{1,}'::lquery
CREATE INDEX idx_entities_scope_paths ON entities USING GIST (scope_paths);

-- Spatial queries: ST_DWithin(location, ...), ST_Within(location, ...).
-- Partial: excludes NULL rows to reduce index size.
CREATE INDEX idx_entities_location ON entities USING GIST (location)
    WHERE location IS NOT NULL;

-- Geography index for native distance queries (metres)
CREATE INDEX idx_entities_location_geo ON entities USING GIST (location_geo);

-- Spatial index for observation space filtering
CREATE INDEX idx_entities_observation_space ON entities USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

-- Spatial index for operation space filtering
CREATE INDEX idx_entities_operation_space ON entities USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

-- Ad-hoc JSONB queries via the q parameter (JSONPath / @> operators)
CREATE INDEX idx_entities_payload ON entities USING GIN (payload);

-- Efficient expiration worker lookup
CREATE INDEX idx_entities_expires_at ON entities(expires_at) WHERE expires_at IS NOT NULL;

-- Temporal system-attribute queries (timerel on modifiedAt)
CREATE INDEX idx_entities_modified_at ON entities(modified_at);
