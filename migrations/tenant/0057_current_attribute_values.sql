-- ============================================================================
-- 0057: The current attribute-value projection, keyed on the entity surrogate
-- ============================================================================
-- One row per (Entity, attribute name, datasetId) -- the identity clause 4.5.5
-- defines -- holding the value that attribute currently has. It replaces
-- `attribute_current`, and the difference is not a rename:
--
--   * It is keyed on `entity_key`, not on a temporal catalogue UUID. The
--     projection is current Entity state, so its only ownership edge is to
--     `entities`. It has no foreign key to anything temporal, and 0045 already
--     made temporal state independent in the other direction: Core Entity
--     deletion (clause 5.6.6) leaves history standing. Deleting an Entity now
--     cascades through exactly one edge.
--   * It carries `attribute_kind` and `value_kind`, the same migration-owned
--     compact registries the rebuilt history carries (0048):
--
--       attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--                       4 VocabProperty, 5 LanguageProperty, 6 JsonProperty,
--                       7 ListProperty, 8 ListRelationship
--       value_kind:     1 Number, 2 String, 3 Boolean, 4 Geometry, 5 Object,
--                       6 DateTime, 7 Uri
--
--     Value kind 0, the tombstone, is permanently reserved and NEVER appears
--     here. A deleted attribute is an absent row, not a row carrying a
--     sentinel. `chk_current_attribute_values_value_kind_column` makes both
--     halves of that unrepresentable: it pins the kind to the populated column
--     and admits no arm for 0. The `urn:ngsi-ld:null` string the old projection
--     stored for a deletion was never a value a client wrote, and a predicate
--     over it had to be excluded by every reader by hand.
--   * `value_number` is `NUMERIC`, not `DOUBLE PRECISION`. JSONB stores numbers
--     as exact numerics, so the old projection held a rounded copy and every
--     comparison beyond 2^53 had to be routed to an unindexed payload scan to
--     stay correct.
--
-- No primary key, deliberately. The natural grain contains a nullable
-- `dataset_id` -- clause 4.5.5 makes the default instance the one with no
-- datasetId -- so the identity is a `UNIQUE NULLS NOT DISTINCT` constraint,
-- which a primary key cannot be. Consequence, recorded here so it is a decision
-- rather than a discovery: under logical replication or change data capture this
-- table needs `REPLICA IDENTITY FULL` or a synthetic key. `REPLICA IDENTITY
-- USING INDEX` cannot be pointed at the grain constraint, because that requires
-- every indexed column to be NOT NULL. Neither is set now -- `FULL` writes the
-- whole old tuple to WAL on every update, and nothing replicates this schema
-- logically today.
--
-- Column order is alignment order: not-null fixed-width descending, then
-- nullable fixed-width, then varlena, LZ4 on each varlena column.
--
-- fillfactor and autovacuum. Every update to a value column also updates a
-- partial index keyed on it, so heap-only-tuple updates are disqualified for
-- exactly the updates this table receives; `modified_at` is not indexed and so
-- contributes nothing to that. The fillfactor keeps the new tuple version on its
-- own page more often, and vacuum cadence is the primary defence, as it is for
-- the queue relations 0038 tuned.
--
-- Every text column here is `COLLATE "C"`, `value_string` included. Clause 4.9
-- orders string values with `<`, `>`, `<=` and `>=` and clause 4.5.19 takes
-- `min` and `max` over them, so under a locale collation the same `q` predicate
-- would select different Entities on two clusters holding identical data, and
-- current state would disagree with the `attribute_history.value_string`
-- aggregation that answers the temporal form of the same question. It also
-- decides the index set below: a locale-collated column plans an anchored
-- `LIKE` as a filter rather than as B-tree range bounds unless a separate
-- `text_pattern_ops` index exists, which is exactly the second index the
-- byte-wise declaration is chosen to avoid.
-- `docs/text-collation-invariant.md` states the invariant for the whole schema.
--
-- Index set, one named consumer each:
--
--   * the grain constraint -- the upsert conflict target of the synchronous
--     write, the cascade check behind the `entity_key` foreign key, and the
--     driving access path of every entity-correlated `q` probe, which reads
--     `WHERE entity_key = <outer> AND attribute_name_id = $k`.
--   * one partial B-tree per typed column, led by `attribute_name_id`, for the
--     direction the planner drives from the attribute side -- a `q` predicate
--     selective enough to beat the entity scan.
--   * NO pattern-operator opclass anywhere. `value_string` and `value_uri` are
--     `COLLATE "C"`, so the default B-tree opclass already compares byte-wise
--     and already serves anchored `LIKE` and anchored regex prefixes; a
--     `text_pattern_ops` copy of the same index would serve nothing the default
--     one does not, and would not serve the `=`, `<` and `>` comparisons clause
--     4.9 actually compiles, which are the majority of the traffic.
--   * one trigram GIN on `value_string`, for the unanchored POSIX patterns
--     `~=` / `!~=` compile to (clause 4.9), which no B-tree can serve. It
--     declares `fastupdate = off`: the projection is now written inside the
--     Entity transaction, and an unbounded GIN pending list turns into
--     unpredictable flush latency on the request path.
--   * no trigram on `value_uri`. A pattern predicate tests the Relationship
--     object as well as the string value, but always as the second disjunct of
--     a test already bounded by `attribute_name_id` and, in the correlated
--     shape, by `entity_key`; a second trigram index would be written on every
--     Relationship write to serve a disjunct nothing drives from. It gets added
--     when a plan is measured wanting it.
--   * one composite GiST on `(attribute_name_id, value_geo)` for clause 4.10
--     geo-queries against a named GeoProperty, and one partial B-tree on
--     `(attribute_name_id, entity_key)` for the correlated instance lookup and
--     the clause 4.23 distance ordering, which is 0033's index re-keyed onto the
--     surrogate. 0033 measured the second at 146 ms against a cancelled 25 s.
--   * NO GIN on `value_object`. Nothing compiles a containment or key-existence
--     predicate against it: `q` routes every compound value path
--     (`address[city]`) and every sub-attribute to the payload fallback, and the
--     projection is only ever compared against a scalar. An index with no
--     consumer is what 0024, 0025 and 0027 removed. When a JSON sub-value
--     predicate is compiled, it is `jsonb_path_ops` if that predicate is
--     containment-only, and the default class only if a key-existence operator
--     is compiled.
--   * `value_bool`'s index is the one entry here whose value is unproven; it is
--     kept at parity with the table it replaces, and removing it is a
--     measurement rather than part of this rebuild.
--
-- Extended statistics. The planner assumes `attribute_name_id`, `value_kind` and
-- `entity_key` are independent. They are strongly correlated -- an attribute
-- name has one value shape in practice, and an Entity carries a stable set of
-- attribute names -- so a compiled `q` predicate is misestimated in the
-- direction that picks the wrong join order. Dependency and n-distinct
-- statistics on both pairs are what correct it.
--
-- Backfill source. `attribute_current` is the source, not `attribute_history`.
-- History is not a statement about current state: 0045 made temporal evolution
-- independent, so it holds streams for Entities that no longer exist and
-- instances the Temporal API appended for attributes current state never had.
-- Rebuilding "current" from it would invent current values. Every column
-- therefore comes from `attribute_current`, with exactly one exception --
-- `attribute_kind`, which the old projection never carried. It is read from the
-- newest history row of the stream the projection row already names
-- (`attribute_current.tea_id` is a `stream_id`: 0050 preserved legacy catalogue
-- UUIDs verbatim, and every stream minted since is written there directly), the
-- same correlation 0042 used to source `unit_code_id`.
--
-- Two documented losses, both inherited rather than introduced:
--
--   * `value_number` widens through `double precision::numeric`, which routes
--     via the shortest round-trip text form of the double -- the identical cast
--     0051 applied to the history backfill. The exact value returns on the next
--     write of that attribute. It cannot be recovered from history either,
--     because the row history holds may be a different instance.
--   * `attribute_kind` on migrated rows is the kind of the newest history row of
--     the stream. That is the instance the projection mirrors, so it is right
--     except where a write landed in history but never reached the old
--     asynchronous projection -- precisely the staleness this rebuild ends.
--
-- Rows whose `value_string` is a stored null representation are deletion
-- tombstones in the old encoding and are dropped, not translated: value kind 0
-- does not exist here, and the attribute they describe is deleted. Both
-- spellings the legacy writer ever produced are recognised, as in 0051.
-- ============================================================================

SELECT CAST('attribute_current row for entity ' || ac.entity_id
            || ' has no entities row' AS SMALLINT)
FROM attribute_current AS ac
WHERE NOT EXISTS (SELECT 1 FROM entities AS e WHERE e.id = ac.entity_id)
LIMIT 1;

SELECT CAST('attribute_current row for stream ' || ac.tea_id::text
            || ' populates no single typed value column' AS SMALLINT)
FROM attribute_current AS ac
WHERE num_nonnulls(ac.value_number, ac.value_string, ac.value_bool, ac.value_geo,
                   ac.value_object, ac.value_datetime, ac.value_uri) <> 1
LIMIT 1;

SELECT CAST('attribute_current row for stream ' || ac.tea_id::text
            || ' has no attribute_history row to take an attribute kind from' AS SMALLINT)
FROM attribute_current AS ac
WHERE ac.value_string IS DISTINCT FROM 'urn:ngsi-ld:null'
  AND ac.value_string IS DISTINCT FROM 'https://uri.etsi.org/ngsi-ld/null'
  AND NOT EXISTS (SELECT 1 FROM attribute_history AS ah WHERE ah.stream_id = ac.tea_id)
LIMIT 1;

SELECT CAST('attribute_current repeats the clause 4.5.5 grain for entity ' || ac.entity_id AS SMALLINT)
FROM attribute_current AS ac
WHERE ac.value_string IS DISTINCT FROM 'urn:ngsi-ld:null'
  AND ac.value_string IS DISTINCT FROM 'https://uri.etsi.org/ngsi-ld/null'
GROUP BY ac.entity_id, ac.attribute_name_id, ac.dataset_id
HAVING count(*) > 1
LIMIT 1;

CREATE TABLE current_attribute_values (
    entity_key BIGINT NOT NULL REFERENCES entities(entity_key) ON DELETE CASCADE,
    modified_at TIMESTAMPTZ NOT NULL,
    attribute_name_id INTEGER NOT NULL REFERENCES attribute_names(id),
    attribute_kind SMALLINT NOT NULL,
    value_kind SMALLINT NOT NULL,
    value_datetime TIMESTAMPTZ,
    unit_code_id SMALLINT REFERENCES unit_codes(id),
    value_bool BOOLEAN,
    dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    value_number NUMERIC COMPRESSION lz4,
    value_string TEXT COMPRESSION lz4 COLLATE "C",
    value_geo geometry COMPRESSION lz4,
    value_object JSONB COMPRESSION lz4,
    value_uri TEXT COMPRESSION lz4 COLLATE "C",
    CONSTRAINT uq_current_attribute_values_grain
        UNIQUE NULLS NOT DISTINCT (entity_key, attribute_name_id, dataset_id),
    CONSTRAINT chk_current_attribute_values_attribute_kind
        CHECK (attribute_kind BETWEEN 1 AND 8),
    CONSTRAINT chk_current_attribute_values_single_value CHECK (
        num_nonnulls(value_number, value_string, value_bool, value_geo,
                     value_object, value_datetime, value_uri) = 1),
    CONSTRAINT chk_current_attribute_values_value_kind_column CHECK (
        (value_kind = 1 AND value_number IS NOT NULL)
        OR (value_kind = 2 AND value_string IS NOT NULL)
        OR (value_kind = 3 AND value_bool IS NOT NULL)
        OR (value_kind = 4 AND value_geo IS NOT NULL)
        OR (value_kind = 5 AND value_object IS NOT NULL)
        OR (value_kind = 6 AND value_datetime IS NOT NULL)
        OR (value_kind = 7 AND value_uri IS NOT NULL)),
    CONSTRAINT chk_current_attribute_values_value_geo_valid
        CHECK (value_geo IS NULL OR ST_IsValid(value_geo)),
    CONSTRAINT chk_current_attribute_values_value_geo_srid
        CHECK (value_geo IS NULL OR ST_SRID(value_geo) = 4326),
    CONSTRAINT chk_current_attribute_values_value_geo_zm
        CHECK (value_geo IS NULL OR ST_Zmflag(value_geo) IN (0, 2))
) WITH (
    fillfactor = 90,
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

INSERT INTO current_attribute_values (
    entity_key, modified_at, attribute_name_id, attribute_kind, value_kind,
    value_datetime, unit_code_id, value_bool,
    dataset_id, value_number, value_string, value_geo, value_object, value_uri)
SELECT
    e.entity_key,
    ac.modified_at,
    ac.attribute_name_id,
    newest.attribute_kind,
    (CASE
        WHEN ac.value_number IS NOT NULL THEN 1
        WHEN ac.value_string IS NOT NULL THEN 2
        WHEN ac.value_bool IS NOT NULL THEN 3
        WHEN ac.value_geo IS NOT NULL THEN 4
        WHEN ac.value_object IS NOT NULL THEN 5
        WHEN ac.value_datetime IS NOT NULL THEN 6
        WHEN ac.value_uri IS NOT NULL THEN 7
     END)::smallint,
    ac.value_datetime,
    ac.unit_code_id,
    ac.value_bool,
    ac.dataset_id,
    ac.value_number::numeric,
    ac.value_string,
    ac.value_geo,
    ac.value_object,
    ac.value_uri
FROM attribute_current AS ac
JOIN entities AS e ON e.id = ac.entity_id
CROSS JOIN LATERAL (
    SELECT ah.attribute_kind
    FROM attribute_history AS ah
    WHERE ah.stream_id = ac.tea_id
    ORDER BY ah.recorded_at DESC
    LIMIT 1) AS newest
WHERE ac.value_string IS DISTINCT FROM 'urn:ngsi-ld:null'
  AND ac.value_string IS DISTINCT FROM 'https://uri.etsi.org/ngsi-ld/null'
ORDER BY e.entity_key, ac.attribute_name_id, ac.dataset_id;

SELECT CAST('current_attribute_values backfill produced ' || rebuilt.n::text
            || ' rows from ' || source.n::text || ' non-tombstone projection rows' AS SMALLINT)
FROM (SELECT count(*) AS n FROM attribute_current
      WHERE value_string IS DISTINCT FROM 'urn:ngsi-ld:null'
        AND value_string IS DISTINCT FROM 'https://uri.etsi.org/ngsi-ld/null') AS source,
     (SELECT count(*) AS n FROM current_attribute_values) AS rebuilt
WHERE source.n <> rebuilt.n;

SELECT CAST('current_attribute_values backfill diverges on entity ' || ac.entity_id
            || ' attribute ' || ac.attribute_name_id::text AS SMALLINT)
FROM attribute_current AS ac
JOIN entities AS e ON e.id = ac.entity_id
WHERE ac.value_string IS DISTINCT FROM 'urn:ngsi-ld:null'
  AND ac.value_string IS DISTINCT FROM 'https://uri.etsi.org/ngsi-ld/null'
  AND NOT EXISTS (
      SELECT 1 FROM current_attribute_values AS c
      WHERE c.entity_key = e.entity_key
        AND c.attribute_name_id = ac.attribute_name_id
        AND c.dataset_id IS NOT DISTINCT FROM ac.dataset_id
        AND c.modified_at = ac.modified_at
        AND c.unit_code_id IS NOT DISTINCT FROM ac.unit_code_id
        AND c.value_number IS NOT DISTINCT FROM ac.value_number::numeric
        AND c.value_string IS NOT DISTINCT FROM ac.value_string
        AND c.value_bool IS NOT DISTINCT FROM ac.value_bool
        AND ST_AsEWKB(c.value_geo) IS NOT DISTINCT FROM ST_AsEWKB(ac.value_geo)
        AND c.value_object IS NOT DISTINCT FROM ac.value_object
        AND c.value_datetime IS NOT DISTINCT FROM ac.value_datetime
        AND c.value_uri IS NOT DISTINCT FROM ac.value_uri)
LIMIT 1;

CREATE INDEX idx_cav_attr_number
    ON current_attribute_values (attribute_name_id, value_number)
    WHERE value_number IS NOT NULL;

CREATE INDEX idx_cav_attr_string
    ON current_attribute_values (attribute_name_id, value_string)
    WHERE value_string IS NOT NULL;

CREATE INDEX idx_cav_string_trgm
    ON current_attribute_values USING GIN (value_string gin_trgm_ops)
    WITH (fastupdate = off)
    WHERE value_string IS NOT NULL;

CREATE INDEX idx_cav_attr_bool
    ON current_attribute_values (attribute_name_id, value_bool)
    WHERE value_bool IS NOT NULL;

CREATE INDEX idx_cav_attr_datetime
    ON current_attribute_values (attribute_name_id, value_datetime)
    WHERE value_datetime IS NOT NULL;

CREATE INDEX idx_cav_attr_uri
    ON current_attribute_values (attribute_name_id, value_uri)
    WHERE value_uri IS NOT NULL;

CREATE INDEX idx_cav_attr_geo
    ON current_attribute_values USING GIST (attribute_name_id, value_geo)
    WHERE value_geo IS NOT NULL;

CREATE INDEX idx_cav_attr_entity_geo
    ON current_attribute_values (attribute_name_id, entity_key)
    WHERE value_geo IS NOT NULL;

CREATE STATISTICS stx_cav_attribute_value_kind (dependencies, ndistinct)
    ON attribute_name_id, value_kind
    FROM current_attribute_values;

CREATE STATISTICS stx_cav_entity_attribute (dependencies, ndistinct)
    ON entity_key, attribute_name_id
    FROM current_attribute_values;
