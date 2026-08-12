-- ============================================================================
-- 0056: Give current Entity state a narrow surrogate identity
-- ============================================================================
-- `entities.id` is an externally meaningful NGSI-LD URI, and until now it was
-- also the key every owned child relation carried and joined on. A URI is long,
-- variable-length and locale-collated; a child relation keyed on it repeats the
-- whole string in its heap tuples, in every index on the grain, and in every
-- hash table and sort that crosses the join. `entity_key` replaces it as the
-- internal identity. The external API keeps looking Entities up by `id`, which
-- stays unique and stays indexed -- clause 5.6.5 addresses an Entity by its URI
-- and nothing here changes that.
--
-- `id` becomes `TEXT COLLATE "C"`. Byte-wise comparison gives anchored
-- `idPattern` prefixes (clause 4.9) a plain B-tree range scan without a
-- pattern-operator index, and gives sorts abbreviated keys. `idx_entities_id_trgm`
-- survives for the patterns that carry no usable anchor.
--
-- Why a rebuild rather than ALTER. Three of the four changes force a heap
-- rewrite on their own -- adding an identity column, changing a column's
-- collation, and recompressing `payload` under LZ4, which `SET COMPRESSION`
-- alone does not do to values already stored. The fourth, physical column
-- order, is unreachable by any ALTER at all: `ADD COLUMN` appends, and this
-- schema orders columns by alignment (not-null fixed-width descending, then
-- nullable fixed-width, then varlena). One copy achieves all four; a sequence
-- of ALTERs achieves three of them and costs more.
--
-- The rebuild is safe because nothing else hangs off this table any more. 0054
-- dropped the last trigger on `entities` and the schema owns no functions
-- beyond `geom_to_geog`; 0045 made the temporal catalogue independent, so
-- neither `temporal_entities` nor `temporal_attribute_streams` references
-- current state; the snapshot tables carry `entity_id TEXT` with no foreign key.
-- `attribute_current.entity_id` is the single remaining foreign key onto
-- `entities(id)`, and a foreign key depends on the unique constraint it points
-- at, so the primary key cannot move while it stands. It is dropped here rather
-- than in 0058: `attribute_current` is read once more, as the backfill source in
-- 0057, and is gone two files later. Between this file and that one it has no
-- cascade from `entities`, which is unobservable -- migrations run at tenant
-- startup, before the pool is published and before any request can delete an
-- Entity.
--
-- `entities_pkey` is dropped from the renamed legacy table before the new table
-- is created, because an index name is unique per schema and the new primary key
-- claims that name. The remaining legacy indexes need no such treatment: they
-- disappear with the table, and the copy reads it sequentially once, so dropping
-- them earlier would buy nothing.
--
-- The copy is ordered by `id`. That is the external access path, so it clusters
-- the heap the way lookups and `idPattern` range scans read it, and it makes
-- `entity_key` follow `id` order, which keeps the correlation the extended
-- statistics in 0057 describe. On a large tenant the sort spills to disk; that
-- is a one-time maintenance cost and is called out in the runbook.
--
-- The identity declares `CACHE 32`. Entity creation is batched and concurrent
-- (`create_batch_via_copy_in_tx` inserts a chunk per connection), and an
-- uncached identity takes the sequence's lock once per row. Thirty-two is a
-- deliberate default rather than a measurement, and the gaps a cached sequence
-- leaves after a restart are meaningless for a surrogate that is never rendered.
--
-- fillfactor and autovacuum. `entities` is rewritten on every attribute write,
-- so it is a hot-update relation by traffic. It is not one by eligibility:
-- heap-only-tuple updates are disqualified whenever a modified column appears in
-- an index key, expression or partial predicate, and both `modified_at`
-- (`idx_entities_modified_at`) and `payload` (`idx_entities_payload_root_keys`,
-- a GIN index on the expression `payload -> 0`) change on every write. The
-- fillfactor below still keeps a new tuple version on its original page more
-- often, and the autovacuum settings are the real defence. Whether dropping
-- either index to buy HOT eligibility is worth its access path is a measurement,
-- not a migration.
--
-- Geometry columns stay typmod-free with explicit SRID and Zm-flag checks rather
-- than `GEOMETRY(Geometry, 4326)`: 0032 established that no typmod spelling
-- accepts XY and XYZ in one column, which RFC 7946 clause 3.1.1 altitudes
-- require.
--
-- `location_geo` is a stored generated column and is therefore absent from the
-- insert column list; PostgreSQL computes it per row from `location`.
--
-- `created_at` and `modified_at` keep the `DEFAULT NOW()` 0005 gave them. The
-- entity insert statements name neither column, so the default is what supplies
-- them; the copy below overrides it with each legacy row's own instants, which
-- is why the rebuild does not restamp every entity as written now.
-- ============================================================================

SELECT CAST('entity ' || e.id || ' carries no type and cannot satisfy the non-empty types invariant' AS SMALLINT)
FROM entities AS e
WHERE cardinality(e.types) = 0
LIMIT 1;

ALTER TABLE attribute_current DROP CONSTRAINT attribute_current_entity_id_fkey;

ALTER TABLE entities RENAME TO entities_legacy;

ALTER TABLE entities_legacy DROP CONSTRAINT entities_pkey;

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

INSERT INTO entities (
    created_at, modified_at, expires_at, id, types, scope_paths, payload,
    location, observation_space, operation_space)
SELECT e.created_at, e.modified_at, e.expires_at, e.id, e.types, e.scope_paths, e.payload,
       e.location, e.observation_space, e.operation_space
FROM entities_legacy AS e
ORDER BY e.id;

SELECT CAST('entities rebuild produced ' || rebuilt.n::text
            || ' rows from ' || legacy.n::text AS SMALLINT)
FROM (SELECT count(*) AS n FROM entities_legacy) AS legacy,
     (SELECT count(*) AS n FROM entities) AS rebuilt
WHERE legacy.n <> rebuilt.n;

SELECT CAST('entities rebuild lost entity ' || e.id AS SMALLINT)
FROM entities_legacy AS e
WHERE NOT EXISTS (SELECT 1 FROM entities AS r WHERE r.id = e.id)
LIMIT 1;

DROP TABLE entities_legacy;

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

CREATE INDEX idx_entities_payload_root_keys ON entities USING GIN ((payload -> 0));

ALTER TABLE entities ALTER COLUMN types SET STATISTICS 1000;
