-- ============================================================================
-- 0012: Context Source Registrations (CSR) and their delivery state
-- ============================================================================
-- NGSI-LD Context Source Registration management (spec clause 5.2.9,
-- operations 5.9.x / 5.10.x).
--
-- A CSR describes what information a remote Context Source can provide.
-- The broker uses this during distributed operations (clause 4.3.3/4.3.4)
-- to discover which Context Sources to query for entities that are not
-- stored locally. Registrations are tenant-global: a Snapshot branch has no
-- registrations of its own.
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
-- There is no `type` column. Every registration's type is
-- `ContextSourceRegistration` (clause 5.2.9); the broker renders the constant
-- and a column holding it on every row would store nothing.
--
-- Closed vocabularies are codes of migration-owned registries the broker
-- compiles in (`andromeda-postgresql-codes`), append-only and never
-- renumbered; each check pins its column to the registered range:
--
--   mode (`CsrModeCode`, clause 4.3.6):
--     1 inclusive, 2 exclusive, 3 redirect, 4 auxiliary
--   operations (`CsrOperationCode`, clause 4.20): 1 `createEntity` through
--     48 `redirectionOps`, the five Table 4.20-2 group tokens included as
--     members in their own right. NULL is the absent member, which the broker
--     reads as the clause 5.2.9 default `federationOps`. The same registry and
--     the same check as `registration_information.operations` (0013).
--   csr_delivery_state.status (`CsrStatusCode`, clause 5.2.9 `status`):
--     1 ok, 2 failed
--
-- `scope` is `ltree[]`, the label-path encoding `entity_search.scope_paths`
-- stores Scopes in (clause 4.18), so a clause 4.19 `scopeQ` against
-- registrations compiles to the same `lquery` match current-state entities
-- are selected by, served by the GiST index below.
--
-- Physical shape:
--   * `id` is an identifier, compared for equality and prefix only, so it is
--     `COLLATE "C"`: byte-wise comparison keeps the btree free of any locale.
--   * The JSONB documents and the free-text columns are `COMPRESSION lz4`; a
--     registration is written rarely and read on every distributed operation,
--     and lz4 decompresses far faster than pglz for the same width.
--   * Fixed-width columns lead in descending alignment, then the varlena
--     columns follow.
--
-- `csr_delivery_state` holds the forwarding statistics and health of clause
-- 5.2.9 (`timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`, `status`),
-- split off the registration by write pattern exactly as
-- `subscription_delivery_state` is split off `subscriptions`: every forwarded
-- request moves them, and keeping them on the registration would rewrite its
-- wide JSONB row on every one. One narrow row per registration, created in the
-- registration's own create transaction and reclaimed with it through the
-- cascade, no index but its primary key, fillfactor 50 so every mark is a
-- heap-only tuple update, and the queue autovacuum cadence because the update
-- rate is the forwarding rate. Forwarding and discovery read `status` through
-- a join on the primary key; registrations are catalogue-scale and the test is
-- a residual on rows the routing projection (0013) has already narrowed, so a
-- status index would have no scan to speed up.
-- ============================================================================

CREATE TABLE context_source_registrations (
    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When this registration expires. NULL means no expiry.
    expires_at TIMESTAMPTZ,

    -- Distributed operation mode (`CsrModeCode`, clause 4.3.6)
    mode SMALLINT NOT NULL,

    -- CSR URI, e.g. "urn:ngsi-ld:ContextSourceRegistration:csr001"
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL PRIMARY KEY,

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

    -- Base URI of the Context Source's NGSI-LD API
    endpoint TEXT COMPRESSION lz4 NOT NULL,

    -- Preferred Accept content type for requests to this source
    accept TEXT COMPRESSION lz4,

    -- Subset of API operations the source supports (`CsrOperationCode`,
    -- clause 4.20). NULL means the default "federationOps" set.
    operations SMALLINT[] COMPRESSION lz4,

    -- NGSI-LD Scopes this source has entities for, as label paths (clause 4.18)
    scope ltree[] COMPRESSION lz4,

    -- ISO 8601 duration hint for how often the source updates
    refresh_rate TEXT COMPRESSION lz4,

    -- Additional registration management info (cacheDuration, etc.)
    management JSONB COMPRESSION lz4,

    -- Arbitrary key-value metadata about the Context Source
    context_source_info JSONB COMPRESSION lz4,

    -- Non-standard CSR properties (for response passthrough and q filtering)
    additional_properties JSONB COMPRESSION lz4,

    -- Table 5.2.9-1 `datasetId`: the datasetIds of the attribute instances the
    -- Context Source can provide, each a URI or the `@none` token naming the
    -- default instance (clause 4.5.5). A typed, specification-defined member with
    -- its own column; no index, since nothing preselects registrations by dataset.
    dataset_id TEXT[] COMPRESSION lz4,

    -- The @context URI associated with this registration
    jsonld_context TEXT COMPRESSION lz4,

    -- Optional human-readable alias for the context source
    context_source_alias TEXT COMPRESSION lz4,

    CONSTRAINT chk_csr_mode CHECK (mode BETWEEN 1 AND 4),
    CONSTRAINT chk_csr_operations_registered CHECK (
        operations IS NULL
        OR (cardinality(operations) > 0
            AND array_position(operations, NULL) IS NULL
            AND 1 <= ALL (operations)
            AND 48 >= ALL (operations))),

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

-- Scope matching for a clause 4.19 `scopeQ` against registrations.
CREATE INDEX idx_csr_scope ON context_source_registrations USING GIST (scope);

-- The expiry sweep reads only registrations that carry an expiry.
CREATE INDEX idx_csr_expires_at ON context_source_registrations (expires_at)
    WHERE expires_at IS NOT NULL;

CREATE TABLE csr_delivery_state (
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    times_sent INTEGER NOT NULL DEFAULT 0,
    times_failed INTEGER NOT NULL DEFAULT 0,
    -- Health status (`CsrStatusCode`)
    status SMALLINT NOT NULL,
    registration_id TEXT COLLATE "C" PRIMARY KEY
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    -- The floor is zero for the same reason as `subscription_delivery_state`:
    -- a counter that has not moved is rendered as an absent member, but the
    -- column has to hold something before it is first advanced.
    CONSTRAINT chk_csr_delivery_state_times_sent_non_negative CHECK (times_sent >= 0),
    CONSTRAINT chk_csr_delivery_state_times_failed_non_negative CHECK (times_failed >= 0),
    CONSTRAINT chk_csr_delivery_state_status CHECK (status BETWEEN 1 AND 2)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);
