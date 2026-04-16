-- ============================================================================
-- 015: Attribute Current -- Flat Current-State Projection
-- ============================================================================
-- One row per (entity_id, attribute_name_id, dataset_id) -- i.e. one row per
-- TEA -- holding the latest typed value for that attribute instance.
--
-- Purpose: make q-filter comparisons like `temperature>50` or
-- `brandName=="Mercedes"` index-bound. The canonical payload JSONB is the
-- source of truth, but every value predicate against it currently compiles to
-- `jsonb_path_query_first(payload, ...)::numeric op $N` which is functional,
-- unindexable, and forces a sequential scan over `entities`.
--
-- Each row stores:
--   * a typed value in exactly one of value_{number,string,bool,geo,object,
--     datetime,uri} (CHECK enforces single-populated column),
--   * the attribute_name_id dictionary key and dataset_id for direct look-up,
--   * the entity_id (denormalized via a foreign key to `entities.id` so that
--     joins back to the entity table stay cheap on the hot read path).
--
-- Lifetime: maintained by the Rust write path alongside `attribute_history`
-- inserts (see TemporalRepository::sync_attribute_current). This migration
-- also performs an initial backfill from the latest observation per TEA so
-- the table is immediately consistent with existing data.
--
-- The tea_id is the PK and cascades on TEA deletion; cascading on entity
-- deletion is indirect via the TEA FK cascade.
-- ============================================================================

CREATE TABLE attribute_current (
    -- PK: each TEA has exactly one current projection row.
    tea_id UUID PRIMARY KEY
        REFERENCES temporal_entity_attributes(id) ON DELETE CASCADE,

    -- Denormalized pointers back to the TEA catalog for fast filtered joins.
    entity_id TEXT NOT NULL
        REFERENCES entities(id) ON DELETE CASCADE,
    attribute_name_id SMALLINT NOT NULL
        REFERENCES attribute_names(id),
    dataset_id TEXT,

    -- Mirrors temporal_entity_attributes.value_type so the reader knows which
    -- typed column is populated without a JOIN.
    value_type TEXT NOT NULL CHECK (value_type IN (
        'Number', 'String', 'Boolean', 'Geometry', 'Object', 'DateTime', 'Uri'
    )),

    -- Typed value columns -- exactly one is populated per row.
    value_number DOUBLE PRECISION,
    value_string TEXT,
    value_bool BOOLEAN,
    value_geo GEOMETRY(Geometry, 4326),
    value_object JSONB,
    value_datetime TIMESTAMPTZ,
    value_uri TEXT,

    -- Server-side modification time of the current value.
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_attribute_current_single_value CHECK (
        num_nonnulls(
            value_number, value_string, value_bool, value_geo,
            value_object, value_datetime, value_uri
        ) = 1
    ),

    CONSTRAINT chk_attribute_current_value_geo_valid
        CHECK (value_geo IS NULL OR ST_IsValid(value_geo))
);

-- Primary filtered-join path from entities into the projection.
CREATE INDEX idx_ac_entity_attr
    ON attribute_current(entity_id, attribute_name_id);

-- Typed per-column partial indexes -- only the populated column indexes each
-- row, so each index is as large as the number of rows of that value type.
CREATE INDEX idx_ac_attr_num
    ON attribute_current(attribute_name_id, value_number)
    WHERE value_number IS NOT NULL;

CREATE INDEX idx_ac_attr_str
    ON attribute_current(attribute_name_id, value_string text_pattern_ops)
    WHERE value_string IS NOT NULL;

CREATE INDEX idx_ac_attr_str_trgm
    ON attribute_current USING GIN (value_string gin_trgm_ops)
    WHERE value_string IS NOT NULL;

CREATE INDEX idx_ac_attr_bool
    ON attribute_current(attribute_name_id, value_bool)
    WHERE value_bool IS NOT NULL;

CREATE INDEX idx_ac_attr_datetime
    ON attribute_current(attribute_name_id, value_datetime)
    WHERE value_datetime IS NOT NULL;

CREATE INDEX idx_ac_attr_uri
    ON attribute_current(attribute_name_id, value_uri text_pattern_ops)
    WHERE value_uri IS NOT NULL;

CREATE INDEX idx_ac_attr_geo
    ON attribute_current USING GIST (attribute_name_id, value_geo)
    WHERE value_geo IS NOT NULL;

CREATE INDEX idx_ac_value_object
    ON attribute_current USING GIN (value_object jsonb_path_ops)
    WHERE value_object IS NOT NULL;

-- ============================================================================
-- Backfill: populate one row per TEA from the most recent attribute_history
-- observation. Uses DISTINCT ON over (tea_id) ORDER BY time DESC to pick the
-- latest row without an expensive window function.
-- ============================================================================

INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT DISTINCT ON (ah.tea_id)
    ah.tea_id,
    tea.entity_id,
    tea.attribute_name_id,
    tea.dataset_id,
    tea.value_type,
    ah.value_number,
    ah.value_string,
    ah.value_bool,
    ah.value_geo,
    ah.value_object,
    ah.value_datetime,
    ah.value_uri,
    ah.time
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
WHERE num_nonnulls(
        ah.value_number, ah.value_string, ah.value_bool, ah.value_geo,
        ah.value_object, ah.value_datetime, ah.value_uri
    ) = 1
ORDER BY ah.tea_id, ah.time DESC;
