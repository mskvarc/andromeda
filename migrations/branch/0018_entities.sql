-- ============================================================================
-- 0018: Entities -- current Entity state, split by how it is written and read
-- ============================================================================
-- One Entity is three rows in three relations that share the surrogate
-- `entity_key`, and the split follows the write pattern rather than the data
-- model. Every attribute write rewrites the payload; almost none of them
-- changes the entity's types, scopes or geometries; and the identity of the
-- row never changes at all. A modified column appearing in any index key or
-- expression disqualifies a heap-only-tuple update, so the payload lives apart
-- from every indexed member: a payload write stays HOT-eligible and leaves the
-- type, scope and geometry index entries of the entity untouched.
--
-- `entities` is identity and lifecycle: the surrogate key, the URI, when the
-- row was created, and the two expiry instants the sweeper reads. `id` is the
-- externally meaningful NGSI-LD URI (clause 5.6.5), stays unique and stays
-- indexed, and is what the API addresses an Entity by. `id` is `TEXT COLLATE
-- "C"`: byte-wise comparison gives anchored `idPattern` prefixes (clause 4.9) a
-- plain B-tree range scan without a pattern-operator index, and gives sorts
-- abbreviated keys. `idx_entities_id_trgm` covers the patterns that carry no
-- usable anchor. `id` is bounded at 1024 UTF-8 octets, the identifier bound
-- (`IDENTIFIER_BYTE_LIMIT` in `andromeda-ngsi-ld-lexicon`) every entity id is
-- parsed under, which keeps `uq_entities_id` well inside the 2704 bytes a
-- btree entry may take. The identity declares `CACHE 32`: entity creation is batched
-- and concurrent, an uncached identity takes the sequence's lock once per row,
-- and the gaps a cached sequence leaves after a restart are meaningless for a
-- surrogate that is never rendered.
--
-- `expires_at` is the Entity-level `expiresAt` (clause 4.22).
-- `min_attribute_expires_at` is the earliest `expiresAt` any attribute
-- instance of the payload carries, computed by the broker from the expanded
-- payload on every write and NULL when no instance expires. It exists so the
-- attribute-expiry sweep is an index range scan over the entities that hold a
-- due instance instead of a walk of every payload; the sweep re-reads the
-- payload of each candidate and decides per instance. Both are timestamptz --
-- pass-by-value, never indexed by expression -- so writing them keeps the
-- update HOT-eligible, and both indexes are partial because most rows carry
-- neither.
--
-- `entity_documents` is the payload and its version token. `modified_at`
-- (clause 4.8 `modifiedAt`) lives here and not on `entities` because it moves
-- with the payload: it is the compare-and-set token an optimistic overwrite
-- states, and it is stamped by the same statement that writes the payload.
-- This relation carries no index other than its primary key, on purpose. Every
-- payload write is an update of this row, and with nothing indexed but the
-- immutable key every one of those updates is HOT-eligible; the fillfactor
-- leaves the room on each page for the new tuple version to land beside the
-- old one. Adding an index to this relation, on any column, is what would
-- undo that -- do not.
--
-- `entity_search` is what the query axes filter on: the interned types
-- (clause 4.17), the interned names of the attributes the payload carries
-- (clause 5.5.9 `attrs`), the scope paths (clause 4.18) and the three
-- hoisted geometries (clauses 4.10 and 4.5.4). Every index the query planner
-- uses to find entities is on this relation, and the write path updates its
-- row only when one of these projected columns actually changed, so a
-- value-only write never touches a search index. `type_ids` and
-- `attribute_name_ids` are `INTEGER[]` of `entity_type_names` and
-- `attribute_names` keys (migration 0008) rather than arrays of IRIs: a type
-- selection or an `attrs` term compiles to `column @> ARRAY[key]`, served by
-- the GIN indexes below, at a few bytes per element instead of an IRI per
-- element. The keys carry no foreign key -- an array element cannot be one --
-- and are minted in the same transaction as the row that references them.
-- `location_expires_at` is the latest `expiresAt` among the payload's
-- `location` instances, NULL when any instance carries none: a geo query
-- against the default GeoProperty excludes an entity whose every `location`
-- instance has expired (clause 4.22) by reading this column rather than
-- walking the payload.
--
-- Geometry columns are typmod-free `geometry` with explicit SRID and Zm-flag
-- checks rather than `GEOMETRY(Geometry, 4326)`: no typmod spelling accepts XY
-- and XYZ in one column, which RFC 7946 clause 3.1.1 altitudes require.
-- `location_geo` is a stored generated column derived from `location` via
-- `geom_to_geog`, computed by PostgreSQL and never named in an insert.
--
-- Autovacuum is set aggressively on all three: the workload updates rather than
-- inserts, and dead tuples on a hot page defeat HOT as surely as an index does.
-- `entity_search` keeps the default fillfactor because its updates are rare and
-- rewrite indexed columns anyway.
--
-- Column order is alignment order in each relation: not-null fixed-width
-- columns first, descending in width, then nullable fixed-width, then varlena,
-- LZ4 on each varlena column.
--
-- Branch relation: this file is in `migrations/branch/`, so it creates the live
-- relations in `public` as part of the tenant migration set and creates and
-- upgrades the same relations inside every Snapshot schema under the branch
-- ledger. Dictionary, unit-code and function references resolve to `public`
-- through the search path; every other reference resolves to the branch.
-- ============================================================================

CREATE TABLE entities (
    entity_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
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

CREATE TABLE entity_documents (
    entity_key BIGINT NOT NULL,
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

-- No other index on entity_documents: every payload write must stay HOT-eligible.

CREATE TABLE entity_search (
    entity_key BIGINT NOT NULL,
    location_expires_at TIMESTAMPTZ,
    type_ids INTEGER[] NOT NULL,
    attribute_name_ids INTEGER[] NOT NULL,
    scope_paths ltree[] COMPRESSION lz4,
    location geometry COMPRESSION lz4,
    location_geo geography GENERATED ALWAYS AS (geom_to_geog(location)) STORED,
    observation_space geometry COMPRESSION lz4,
    operation_space geometry COMPRESSION lz4,
    CONSTRAINT pk_entity_search PRIMARY KEY (entity_key),
    CONSTRAINT fk_entity_search_entity
        FOREIGN KEY (entity_key)
        REFERENCES entities(entity_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_search_types_present CHECK (cardinality(type_ids) > 0),
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

CREATE INDEX idx_entity_search_type_ids ON entity_search USING GIN (type_ids);

CREATE INDEX idx_entity_search_attribute_name_ids ON entity_search USING GIN (attribute_name_ids);

CREATE INDEX idx_entity_search_scope_paths ON entity_search USING GIST (scope_paths);

CREATE INDEX idx_entity_search_location ON entity_search USING GIST (location)
    WHERE location IS NOT NULL;

CREATE INDEX idx_entity_search_location_geo ON entity_search USING GIST (location_geo)
    WHERE location_geo IS NOT NULL;

CREATE INDEX idx_entity_search_observation_space ON entity_search USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

CREATE INDEX idx_entity_search_operation_space ON entity_search USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

ALTER TABLE entity_search ALTER COLUMN type_ids SET STATISTICS 1000;
