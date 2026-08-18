-- ============================================================================
-- 0015: Context Source Registrations (CSR)
-- ============================================================================
-- NGSI-LD Context Source Registration management (spec clause 5.2.9,
-- operations 5.9.x / 5.10.x).
--
-- A CSR describes what information a remote Context Source can provide.
-- The broker uses this during distributed operations (clause 4.3.3/4.3.4)
-- to discover which Context Sources to query for entities that are not
-- stored locally.
--
-- Spatial columns (PostGIS):
--   * location          -- geographic area the Context Source covers
--   * observation_space -- union of all entities' observationSpace GeoProperties
--   * operation_space   -- union of all entities' operationSpace GeoProperties
--   All three are partial-GIST-indexed (WHERE ... IS NOT NULL) for efficient
--   spatial matching during distributed entity discovery, and are typmod-free
--   `geometry` with explicit SRID and Zm-flag checks rather than
--   `GEOMETRY(Geometry, 4326)`: no typmod spelling accepts XY and XYZ in one
--   column, which RFC 7946 clause 3.1.1 altitudes require.
--
-- JSONB columns for dynamic/nested spec structures:
--   * information:           array of RegistrationInfo, each containing
--                            entities (EntitySelector[]), propertyNames,
--                            relationshipNames
--   * observation_interval:  TimeInterval {startAt, endAt} -- temporal range
--                            for observedAt-based queries
--   * management_interval:   TimeInterval {startAt, endAt} -- temporal range
--                            for createdAt/modifiedAt-based queries
--   * management:            RegistrationManagementInfo (cacheDuration,
--                            reconPeriod, etc.)
--   * context_source_info:   additional key-value metadata about the source
--
-- Physical shape:
--   * `id` is an identifier, compared for equality and prefix only, so it is
--     `COLLATE "C"`: byte-wise comparison keeps the btree free of any locale.
--   * The JSONB documents and the free-text columns are `COMPRESSION lz4`; a
--     registration is written rarely and read on every distributed operation,
--     and lz4 decompresses far faster than pglz for the same width.
--   * The only index beside the primary key and the three spatial ones is a
--     partial btree on `expires_at`, which serves the expiry sweep. Forwarding
--     filters healthy registrations by `status = 'ok'`, but registrations are
--     catalogue-scale and the status test is a residual on rows discovery has
--     already narrowed through the routing projection (0016), so a status
--     index has no scan to speed up.
-- ============================================================================

CREATE TABLE context_source_registrations (
    -- CSR URI, e.g. "urn:ngsi-ld:ContextSourceRegistration:csr001"
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL PRIMARY KEY,

    -- Always "ContextSourceRegistration" per spec
    type TEXT NOT NULL DEFAULT 'ContextSourceRegistration',

    -- Optional human-readable name
    registration_name TEXT COMPRESSION lz4,

    -- Optional description
    description TEXT COMPRESSION lz4,

    -- What the Context Source provides -- array of RegistrationInfo:
    -- [{entities: [{type, id, idPattern}], propertyNames: [...], ...}]
    information JSONB COMPRESSION lz4 NOT NULL,

    -- Multi-tenancy: the tenant identifier for requests to this Context Source.
    -- NULL means the default tenant.
    tenant TEXT COMPRESSION lz4,

    -- Temporal range the source can provide for observedAt-based queries.
    -- Example: {"startAt": "2020-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00Z"}
    observation_interval JSONB COMPRESSION lz4,

    -- Temporal range for createdAt/modifiedAt-based queries
    management_interval JSONB COMPRESSION lz4,

    -- Geographic area covered by this Context Source (WGS 84)
    location geometry,

    -- Union of observation spaces of all entities the source provides
    observation_space geometry,

    -- Union of operation spaces of all entities the source provides
    operation_space geometry,

    -- When this registration expires. NULL means no expiry.
    expires_at TIMESTAMPTZ,

    -- Base URI of the Context Source's NGSI-LD API
    endpoint TEXT COMPRESSION lz4 NOT NULL,

    -- Preferred Accept content type for requests to this source
    accept TEXT COMPRESSION lz4,

    -- Distributed operation mode (clause 4.3.6)
    mode csr_mode NOT NULL DEFAULT 'inclusive',

    -- Subset of API operations the source supports (clause 4.20).
    -- NULL means the default "federationOps" set.
    operations TEXT[] COMPRESSION lz4,

    -- NGSI-LD Scopes this source has entities for
    scope TEXT[] COMPRESSION lz4,

    -- ISO 8601 duration hint for how often the source updates
    refresh_rate TEXT COMPRESSION lz4,

    -- Additional registration management info (cacheDuration, etc.)
    management JSONB COMPRESSION lz4,

    -- Arbitrary key-value metadata about the Context Source
    context_source_info JSONB COMPRESSION lz4,

    -- Non-standard CSR properties (for response passthrough and q filtering)
    additional_properties JSONB COMPRESSION lz4,

    -- The @context URI associated with this registration
    jsonld_context TEXT COMPRESSION lz4,

    -- Health status
    status csr_status NOT NULL DEFAULT 'ok',

    -- Optional human-readable alias for the context source
    context_source_alias TEXT COMPRESSION lz4,

    -- Forwarding statistics
    times_sent INTEGER NOT NULL DEFAULT 0,
    times_failed INTEGER NOT NULL DEFAULT 0,
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,

    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Geometry validity constraints
    CONSTRAINT chk_csr_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    CONSTRAINT chk_csr_location_srid
        CHECK (location IS NULL OR ST_SRID(location) = 4326),
    CONSTRAINT chk_csr_location_zm
        CHECK (location IS NULL OR ST_Zmflag(location) IN (0, 2)),
    CONSTRAINT chk_csr_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    CONSTRAINT chk_csr_observation_space_srid
        CHECK (observation_space IS NULL OR ST_SRID(observation_space) = 4326),
    CONSTRAINT chk_csr_observation_space_zm
        CHECK (observation_space IS NULL OR ST_Zmflag(observation_space) IN (0, 2)),
    CONSTRAINT chk_csr_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space)),
    CONSTRAINT chk_csr_operation_space_srid
        CHECK (operation_space IS NULL OR ST_SRID(operation_space) = 4326),
    CONSTRAINT chk_csr_operation_space_zm
        CHECK (operation_space IS NULL OR ST_Zmflag(operation_space) IN (0, 2))
);

-- Spatial matching for distributed entity discovery (partial: excludes NULL rows)
CREATE INDEX idx_csr_location ON context_source_registrations USING GIST (location)
    WHERE location IS NOT NULL;

-- Spatial index for observation space filtering
CREATE INDEX idx_csr_observation_space ON context_source_registrations USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

-- Spatial index for operation space filtering
CREATE INDEX idx_csr_operation_space ON context_source_registrations USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

-- The expiry sweep reads only registrations that carry an expiry.
CREATE INDEX idx_csr_expires_at ON context_source_registrations (expires_at)
    WHERE expires_at IS NOT NULL;
