-- ============================================================================
-- 0013: The current attribute-value projection, keyed on the entity surrogate
-- ============================================================================
-- One row per (Entity, attribute name, datasetId) -- the identity clause 4.5.5
-- defines -- holding the value that attribute currently has.
--
--   * Keyed on `entity_key`. The projection is current Entity state, so its
--     only ownership edge is to `entities`. It has no foreign key to anything
--     temporal: Core Entity deletion (clause 5.6.6) leaves history standing,
--     and the temporal catalogue is independent in the other direction.
--   * `attribute_kind` and `value_kind` are compact migration-owned codes,
--     the same registries `attribute_history` carries:
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
--     halves of that unrepresentable: it pins the kind to the populated
--     column and admits no arm for 0.
--   * `value_number` is `NUMERIC`, not `DOUBLE PRECISION`. JSONB stores
--     numbers as exact numerics, and a comparison beyond 2^53 stays correct
--     without falling back to an unindexed payload scan.
--
-- No primary key, deliberately. The natural grain contains a nullable
-- `dataset_id` -- clause 4.5.5 makes the default instance the one with no
-- datasetId -- so the identity is a `UNIQUE NULLS NOT DISTINCT` constraint,
-- which a primary key cannot be. Consequence: under logical replication or
-- change data capture this table needs `REPLICA IDENTITY FULL` or a synthetic
-- key. Neither is set now -- nothing replicates this schema logically today.
--
-- Column order is alignment order: not-null fixed-width descending, then
-- nullable fixed-width, then varlena, LZ4 on each varlena column.
--
-- fillfactor and autovacuum. Every update to a value column also updates a
-- partial index keyed on it, so heap-only-tuple updates are disqualified for
-- exactly the updates this table receives. The fillfactor keeps the new
-- tuple version on its own page more often, and vacuum cadence is the
-- primary defence.
--
-- Every text column here is `COLLATE "C"`, `value_string` included. Clause
-- 4.9 orders string values with `<`, `>`, `<=` and `>=` and clause 4.5.19
-- takes `min` and `max` over them, so under a locale collation the same `q`
-- predicate would select different Entities on two clusters holding
-- identical data, and current state would disagree with the
-- `attribute_history.value_string` aggregation that answers the temporal
-- form of the same question. `docs/text-collation-invariant.md` states the
-- invariant for the whole schema.
--
-- Index set, one named consumer each:
--
--   * the grain constraint -- the upsert conflict target of the synchronous
--     write, the cascade check behind the `entity_key` foreign key, and the
--     driving access path of every entity-correlated `q` probe.
--   * one partial B-tree per typed column, led by `attribute_name_id`, for
--     the direction the planner drives from the attribute side.
--   * NO pattern-operator opclass anywhere. `value_string` and `value_uri`
--     are `COLLATE "C"`, so the default B-tree opclass already compares
--     byte-wise and already serves anchored `LIKE` and anchored regex
--     prefixes.
--   * one trigram GIN on `value_string`, for the unanchored POSIX patterns
--     `~=` / `!~=` compile to (clause 4.9), which no B-tree can serve. It
--     declares `fastupdate = off`: the projection is written inside the
--     Entity transaction, and an unbounded GIN pending list turns into
--     unpredictable flush latency on the request path.
--   * no trigram on `value_uri` -- a second trigram index would be written
--     on every Relationship write to serve a disjunct nothing drives from.
--   * one composite GiST on `(attribute_name_id, value_geo)` for clause 4.10
--     geo-queries against a named GeoProperty, and one partial B-tree on
--     `(attribute_name_id, entity_key)` for the correlated instance lookup
--     and the clause 4.23 distance ordering.
--   * NO GIN on `value_object`. Nothing compiles a containment or
--     key-existence predicate against it: `q` routes every compound value
--     path and every sub-attribute to the payload fallback, and the
--     projection is only ever compared against a scalar.
--
-- Extended statistics. The planner assumes `attribute_name_id`, `value_kind`
-- and `entity_key` are independent. They are strongly correlated -- an
-- attribute name has one value shape in practice, and an Entity carries a
-- stable set of attribute names -- so a compiled `q` predicate is
-- misestimated in the direction that picks the wrong join order. Dependency
-- and n-distinct statistics on both pairs are what correct it.
-- ============================================================================

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
