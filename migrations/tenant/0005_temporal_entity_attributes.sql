-- ============================================================================
-- 004: Temporal Entity Attributes (TEA) — Attribute Catalog
-- ============================================================================
-- Normalization layer between entities and the time-series hypertable.
--
-- Instead of storing the full attribute name string and entity ID on every
-- row in attribute_history (potentially millions of rows), we store a
-- compact UUID reference.  This:
--   * Reduces storage by avoiding repeated TEXT in the hot time-series table
--   * Enables efficient JOINs for temporal queries
--   * Maps each attribute to its value_type so Rust knows which typed column
--     to read from attribute_history (Number -> value_number, etc.)
--
-- Attribute name dictionary:
--   With ~30 distinct attribute names across millions of TEA rows, storing a
--   SMALLINT FK instead of repeated 60+ char IRIs saves hundreds of MB in
--   both the table and its UNIQUE constraint index.
--
-- The UNIQUE constraint with NULLS NOT DISTINCT enforces NGSI-LD identity
-- rules: an entity can have multiple instances of the same attribute name
-- only if they carry distinct datasetId values (clause 4.5.5).
-- ============================================================================

-- Attribute name dictionary for normalization.
CREATE TABLE attribute_names (
    id SMALLINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    name TEXT NOT NULL,
    CONSTRAINT uq_attribute_names_name UNIQUE (name)
);

CREATE TABLE temporal_entity_attributes (
    -- Surrogate key -- referenced by attribute_history.tea_id
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- The entity this attribute belongs to
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,

    -- FK into the attribute_names dictionary
    attribute_name_id SMALLINT NOT NULL REFERENCES attribute_names(id),

    -- NGSI-LD attribute meta-type:
    -- "Property", "Relationship", "GeoProperty", "VocabProperty", "LanguageProperty",
    -- "JsonProperty", "ListProperty", "ListRelationship"
    attribute_type TEXT NOT NULL CHECK (attribute_type IN (
        'Property', 'Relationship', 'GeoProperty', 'VocabProperty',
        'LanguageProperty', 'JsonProperty', 'ListProperty', 'ListRelationship'
    )),

    -- Multi-attribute discriminator (clause 4.5.5).
    -- NULL means the default (un-named) instance.
    dataset_id TEXT,

    -- Describes the runtime type of the value stored in attribute_history,
    -- so the Rust layer knows which column to read/write:
    -- "Number", "String", "Boolean", "Geometry", "Object", "DateTime", "Uri"
    value_type TEXT NOT NULL CHECK (value_type IN (
        'Number', 'String', 'Boolean', 'Geometry', 'Object', 'DateTime', 'Uri'
    )),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Enforce NGSI-LD identity: (entity, attribute, datasetId) must be unique.
    -- NULLS NOT DISTINCT treats two NULL dataset_ids as equal.
    UNIQUE NULLS NOT DISTINCT (entity_id, attribute_name_id, dataset_id)
);

-- NOTE: no separate (entity_id, attribute_name_id) index is needed.  The UNIQUE
-- constraint on (entity_id, attribute_name_id, dataset_id) already covers lookups
-- by the leading (entity_id, attribute_name_id) prefix.
