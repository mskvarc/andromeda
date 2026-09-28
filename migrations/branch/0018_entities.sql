-- ============================================================================
-- 0018: Entities
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.7.1, 4.8, 4.10, 4.17, 4.18, 4.22,
--   5.2.23, 5.5.9
--
-- Current Entity state, as three rows in three relations sharing
-- `entity_key`, and one membership row per type the Entity carries. The split
-- follows the write pattern, not the data model: every attribute write
-- rewrites the payload, almost none changes the Entity's types, scopes or
-- geometries, and its identity never changes. The payload therefore lives
-- apart from every indexed column, so a payload write stays a heap-only update
-- and leaves the type, scope and geometry index entries untouched.
-- ============================================================================

-- Identity and lifecycle.
CREATE TABLE entities (
    entity_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    -- The earliest `expiresAt` any attribute instance of the payload carries,
    -- NULL when none expires. The attribute-expiry sweep ranges over it
    -- instead of walking every payload, then re-reads each candidate's
    -- payload and decides per instance.
    min_attribute_expires_at TIMESTAMPTZ,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_entities PRIMARY KEY (entity_key),
    CONSTRAINT uq_entities_id UNIQUE (id),
    CONSTRAINT chk_entities_id_length CHECK (octet_length(id) <= 1024)
) WITH (
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_entities_expires_at ON entities (expires_at)
    WHERE expires_at IS NOT NULL;

CREATE INDEX idx_entities_min_attribute_expires_at ON entities (min_attribute_expires_at)
    WHERE min_attribute_expires_at IS NOT NULL;

CREATE INDEX idx_entities_id_trgm ON entities USING GIN (id gin_trgm_ops);

-- The payload and its version token. It has no index but its primary key,
-- and must never gain one: every payload write is an update of this row, and
-- any index would stop those updates being heap-only.
CREATE TABLE entity_documents (
    entity_key BIGINT NOT NULL,
    -- Clause 4.8 `modifiedAt`, kept here because it moves with the payload:
    -- the same statement stamps both, and it is the compare-and-set token an
    -- optimistic overwrite states.
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload JSONB COMPRESSION lz4 NOT NULL,
    CONSTRAINT pk_entity_documents PRIMARY KEY (entity_key),
    CONSTRAINT fk_entity_documents_entity
        FOREIGN KEY (entity_key)
        REFERENCES entities(entity_key) ON DELETE CASCADE
) WITH (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The query axes: types (clause 4.17), the names of the attributes the
-- payload carries (clause 5.2.23 `attrs`), scopes (clause 4.18) and the three
-- hoisted geometries (clauses 4.7.1 and 4.10). Every index that finds
-- entities is here, and a row is rewritten only when one of these columns
-- changes. It keeps the default fillfactor: its updates are rare, and they
-- rewrite indexed columns anyway.
CREATE TABLE entity_search (
    entity_key BIGINT NOT NULL,
    -- The `xid8` of the transaction that last wrote the row: the insert takes
    -- it from the default and every update sets it. A broker that keeps an
    -- in-memory spatial index reads the rows written at or above the `xmin`
    -- of its previous read's snapshot to catch up with every change,
    -- whichever process committed it; an `xid8` never wraps, and a
    -- transaction below a snapshot's `xmin` had finished before the snapshot
    -- was taken.
    written_xid XID8 NOT NULL DEFAULT pg_current_xact_id(),
    -- The clause 4.22 `expiresAt` of the hoisted default `location` instance,
    -- NULL when it carries none. A geo-query reads the hoisted instance only
    -- while this is NULL or ahead of NOW(): clause 4.22 invalidates that one
    -- instance at its own expiry, whatever the Entity's other `location`
    -- instances carry in `current_attribute_values`.
    location_expires_at TIMESTAMPTZ,
    -- `entity_type_names` keys.
    entity_type_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    -- `attribute_names` keys.
    attribute_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    scope_paths ltree[] COMPRESSION lz4,
    location geometry COMPRESSION lz4,
    location_geography geography COMPRESSION lz4 GENERATED ALWAYS AS (geom_to_geog(location)) STORED,
    observation_space geometry COMPRESSION lz4,
    operation_space geometry COMPRESSION lz4,
    CONSTRAINT pk_entity_search PRIMARY KEY (entity_key),
    CONSTRAINT fk_entity_search_entity
        FOREIGN KEY (entity_key)
        REFERENCES entities(entity_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_search_entity_type_name_keys_present CHECK (cardinality(entity_type_name_keys) > 0),
    CONSTRAINT chk_entity_search_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    CONSTRAINT chk_entity_search_location_srid
        CHECK (location IS NULL OR ST_SRID(location) = 4326),
    CONSTRAINT chk_entity_search_location_zm
        CHECK (location IS NULL OR ST_Zmflag(location) IN (0, 2)),
    CONSTRAINT chk_entity_search_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    CONSTRAINT chk_entity_search_observation_space_srid
        CHECK (observation_space IS NULL OR ST_SRID(observation_space) = 4326),
    CONSTRAINT chk_entity_search_observation_space_zm
        CHECK (observation_space IS NULL OR ST_Zmflag(observation_space) IN (0, 2)),
    CONSTRAINT chk_entity_search_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space)),
    CONSTRAINT chk_entity_search_operation_space_srid
        CHECK (operation_space IS NULL OR ST_SRID(operation_space) = 4326),
    CONSTRAINT chk_entity_search_operation_space_zm
        CHECK (operation_space IS NULL OR ST_Zmflag(operation_space) IN (0, 2))
) WITH (
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

CREATE INDEX idx_entity_search_entity_type_name_keys ON entity_search USING GIN (entity_type_name_keys);

CREATE INDEX idx_entity_search_attribute_name_keys ON entity_search USING GIN (attribute_name_keys);

CREATE INDEX idx_entity_search_scope_paths ON entity_search USING GIST (scope_paths);

CREATE INDEX idx_entity_search_location ON entity_search USING GIST (location)
    WHERE location IS NOT NULL;

CREATE INDEX idx_entity_search_location_geography ON entity_search USING GIST (location_geography)
    WHERE location_geography IS NOT NULL;

CREATE INDEX idx_entity_search_observation_space ON entity_search USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

CREATE INDEX idx_entity_search_operation_space ON entity_search USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

-- The spatial index catch-up: a range scan over recently written rows.
CREATE INDEX idx_entity_search_written_xid ON entity_search (written_xid);

ALTER TABLE entity_search ALTER COLUMN entity_type_name_keys SET STATISTICS 1000;

-- One row per type an Entity carries (clause 4.17 selects on membership),
-- keyed by type and then identifier. A type selection pages by walking one
-- range of this key in the clause 5.5.9 `id` order, reading exactly the rows it
-- skips and keeps. The GIN over `entity_search.entity_type_name_keys` cannot
-- give that order, and a walk of `uq_entities_id` filtered by type reads every
-- Entity whose identifier sorts ahead of the type's first. The key carries
-- `entity_key` so the walk never visits the heap, and the insert-driven vacuum
-- keeps the visibility map current enough for that to hold.
CREATE TABLE entity_type_members (
    entity_key BIGINT NOT NULL,
    entity_type_name_key INTEGER NOT NULL,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_entity_type_members PRIMARY KEY (entity_type_name_key, id) INCLUDE (entity_key),
    CONSTRAINT fk_entity_type_members_entity
        FOREIGN KEY (entity_key)
        REFERENCES entities(entity_key) ON DELETE CASCADE
) WITH (
    autovacuum_vacuum_insert_scale_factor = 0.01,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The cascade from `entities` and a type change find an Entity's rows by key.
CREATE INDEX idx_entity_type_members_entity_key ON entity_type_members (entity_key);
