-- ============================================================================
-- 0012: Context source registrations and their delivery state
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.3.6, 4.5.5, 4.19, 4.20, 5.2.9, 5.9,
--   5.10
--
-- What each remote context source declares it can provide. Distributed
-- operations consult these registrations to decide which context sources a
-- request must reach beyond the entities held locally.
--
-- Codes:
--
--   mode: 1 inclusive, 2 exclusive, 3 redirect, 4 auxiliary
--   operations: 1 createEntity, 2 updateEntity, 3 appendAttrs, 4 updateAttrs,
--     5 deleteAttrs, 6 deleteEntity, 7 createBatch, 8 upsertBatch,
--     9 updateBatch, 10 deleteBatch, 11 upsertTemporal,
--     12 appendAttrsTemporal, 13 deleteAttrsTemporal,
--     14 updateAttrInstanceTemporal, 15 deleteAttrInstanceTemporal,
--     16 deleteTemporal, 17 mergeEntity, 18 replaceEntity, 19 replaceAttrs,
--     20 mergeBatch, 21 purgeEntity, 22 retrieveEntity, 23 queryEntity,
--     24 queryBatch, 25 retrieveTemporal, 26 queryTemporal,
--     27 retrieveEntityTypes, 28 retrieveEntityTypeDetails,
--     29 retrieveEntityTypeInfo, 30 retrieveAttrTypes,
--     31 retrieveAttrTypeDetails, 32 retrieveAttrTypeInfo,
--     33 createSubscription, 34 updateSubscription, 35 retrieveSubscription,
--     36 querySubscription, 37 deleteSubscription, 38 retrieveEntityMap,
--     39 updateEntityMap, 40 deleteEntityMap, 41 createEntityMapQueryEntity,
--     42 createEntityMapQueryTemporal, 43 retrieveContextSourceIdentity,
--     44 federationOps, 45 associationOps, 46 updateOps, 47 retrieveOps,
--     48 redirectionOps
--   registration_delivery_state.status: 1 ok, 2 failed
--
-- The five operation groups of Table 4.20-2 are stored as declared, never
-- expanded into the operations they stand for: expanding them would freeze one
-- reading of that table into every stored row.
--
-- There is no `type` column. Every registration's type is
-- `ContextSourceRegistration` (clause 5.2.9), so the broker renders the
-- constant rather than storing it on every row.
-- ============================================================================

CREATE TABLE context_source_registrations (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- NULL means the registration never expires.
    expires_at TIMESTAMPTZ,
    mode SMALLINT NOT NULL,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    registration_name TEXT COMPRESSION lz4,
    description TEXT COMPRESSION lz4,
    -- The authority for every response body, rendered back verbatim.
    -- `registration_information` is its routing projection.
    information JSONB COMPRESSION lz4 NOT NULL,
    -- The tenant to address at the context source; NULL means its default
    -- tenant.
    tenant TEXT COMPRESSION lz4,
    observation_interval JSONB COMPRESSION lz4,
    management_interval JSONB COMPRESSION lz4,
    location geometry COMPRESSION lz4,
    observation_space geometry COMPRESSION lz4,
    operation_space geometry COMPRESSION lz4,
    endpoint TEXT COMPRESSION lz4 NOT NULL,
    -- The media type the broker sends as `Accept` to this context source.
    accept TEXT COMPRESSION lz4,
    -- NULL is the absent member, which reads as the clause 5.2.9 default,
    -- `federationOps`.
    operations SMALLINT[] COMPRESSION lz4,
    -- Stored in the label-path encoding of `entity_search.scope_paths`, so a
    -- clause 4.19 `scopeQ` against registrations compiles to the same `lquery`
    -- match that selects entities.
    scope ltree[] COMPRESSION lz4,
    refresh_rate TEXT COMPRESSION lz4,
    management JSONB COMPRESSION lz4,
    context_source_info JSONB COMPRESSION lz4,
    -- Members outside Table 5.2.9-1, kept for the representation and for `q`
    -- filtering.
    additional_properties JSONB COMPRESSION lz4,
    -- Table 5.2.9-1 `datasetId`: each element a URI, or `@none` for the
    -- default instance (clause 4.5.5). Unindexed: nothing preselects
    -- registrations by dataset.
    dataset_id TEXT[] COMPRESSION lz4,
    -- The `@context` the registration was created under.
    jsonld_context TEXT COMPRESSION lz4,
    context_source_alias TEXT COMPRESSION lz4,
    CONSTRAINT pk_context_source_registrations PRIMARY KEY (id),
    CONSTRAINT chk_context_source_registrations_id_length CHECK (octet_length(id) <= 1024),
    CONSTRAINT chk_csr_mode CHECK (mode BETWEEN 1 AND 4),
    CONSTRAINT chk_csr_operations CHECK (
        operations IS NULL
        OR (cardinality(operations) > 0
            AND array_position(operations, NULL) IS NULL
            AND 1 <= ALL (operations)
            AND 48 >= ALL (operations))),
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

-- The three spatial indexes preselect registrations by geography for
-- distributed operations and discovery.
CREATE INDEX idx_csr_location ON context_source_registrations USING GIST (location)
    WHERE location IS NOT NULL;

CREATE INDEX idx_csr_observation_space ON context_source_registrations USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

CREATE INDEX idx_csr_operation_space ON context_source_registrations USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

CREATE INDEX idx_csr_scope ON context_source_registrations USING GIST (scope);

CREATE INDEX idx_csr_expires_at ON context_source_registrations (expires_at)
    WHERE expires_at IS NOT NULL;

-- The forwarding statistics and health of clause 5.2.9. Every forwarded
-- request moves them, so they live apart from the registration's wide row.
-- The row is created in the registration's own create transaction. Readers
-- test `status` through a primary-key join on registrations the routing
-- projection has already narrowed, so a `status` index would have no scan to
-- speed up.
CREATE TABLE registration_delivery_state (
    last_success TIMESTAMPTZ,
    last_failure TIMESTAMPTZ,
    times_sent INTEGER NOT NULL DEFAULT 0,
    times_failed INTEGER NOT NULL DEFAULT 0,
    status SMALLINT NOT NULL,
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_registration_delivery_state PRIMARY KEY (registration_id),
    CONSTRAINT fk_registration_delivery_state_registration FOREIGN KEY (registration_id)
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    CONSTRAINT chk_registration_delivery_state_registration_id_length CHECK (octet_length(registration_id) <= 1024),
    -- The counters floor at zero. A counter that has not moved is rendered as
    -- an absent member, but the column holds a value before it first advances.
    CONSTRAINT chk_registration_delivery_state_times_sent_non_negative CHECK (times_sent >= 0),
    CONSTRAINT chk_registration_delivery_state_times_failed_non_negative CHECK (times_failed >= 0),
    CONSTRAINT chk_registration_delivery_state_status CHECK (status BETWEEN 1 AND 2)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);
