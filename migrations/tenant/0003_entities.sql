-- ============================================================================
-- 002: Entity Registry
-- ============================================================================
-- Holds the *current state* of every NGSI-LD Entity.
--
-- Design rationale:
--   The full expanded JSON-LD document lives in `payload` (JSONB), which
--   preserves the dynamic, schema-less nature of NGSI-LD.  Critical search
--   fields are "hoisted" into top-level SQL columns so they can be indexed
--   natively — avoiding slow JSON parsing during filtering.
--
--   • types[]   — GIN-indexed array for fast type-based filtering
--   • scopes[]  — GIN-indexed array for NGSI-LD Scope queries (clause 4.18)
--   • location  — PostGIS geometry extracted from the entity's default
--                  GeoProperty named "location", enabling GIST-indexed
--                  spatial queries (within, near, intersects, etc.)
--   • payload   — GIN-indexed JSONB for ad-hoc JSONPath queries (q filter)
-- ============================================================================

CREATE TABLE entities (
    -- URI identifier, e.g. "urn:ngsi-ld:Vehicle:A1"
    id TEXT NOT NULL PRIMARY KEY,

    -- Entity type(s), e.g. '{"Vehicle", "WasteManagement"}'
    -- An entity may belong to multiple types per NGSI-LD spec.
    types TEXT[] NOT NULL,

    -- NGSI-LD Scopes (clause 4.18) for hierarchical entity structuring,
    -- e.g. '{"/Madrid/Gardens", "/Madrid/Traffic"}'
    scopes TEXT[],

    -- Materialized geometry from the entity's "location" GeoProperty.
    -- Supports Point, Polygon, LineString, etc. in WGS 84 (SRID 4326).
    location GEOMETRY(Geometry, 4326),

    -- Full expanded JSON-LD document — the source of truth for all
    -- attribute data. Rust maps this via sqlx::types::Json<Value>.
    payload JSONB NOT NULL,

    -- Transient storage support (NGSI-LD §4.22).
    -- When set, the entity becomes invalid after this timestamp.
    expires_at TIMESTAMPTZ,

    -- System-managed temporal properties (sysAttrs)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast filtering by entity type(s):  WHERE 'Vehicle' = ANY(types)
CREATE INDEX idx_entities_types ON entities USING GIN (types);

-- Fast filtering by scope(s):  WHERE scopes && ARRAY['/Madrid/Gardens']
CREATE INDEX idx_entities_scopes ON entities USING GIN (scopes);

-- Spatial queries:  ST_DWithin(location, ...), ST_Within(location, ...)
CREATE INDEX idx_entities_location ON entities USING GIST (location);

-- Ad-hoc JSONB queries via the q parameter (JSONPath / @> operators)
CREATE INDEX idx_entities_payload ON entities USING GIN (payload);

-- Efficient expiration worker lookup
CREATE INDEX idx_entities_expires_at ON entities(expires_at) WHERE expires_at IS NOT NULL;

-- Temporal system-attribute queries (timerel on modifiedAt)
CREATE INDEX idx_entities_modified_at ON entities(modified_at);
