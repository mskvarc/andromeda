-- ============================================================================
-- 004: Temporal Entity Attributes (TEA) — Attribute Catalog
-- ============================================================================
-- Normalization layer between entities and the time-series hypertable.
--
-- Instead of storing the full attribute name string and entity ID on every
-- row in attribute_history (potentially millions of rows), we store a
-- compact UUID reference.  This:
--   • Reduces storage by avoiding repeated TEXT in the hot time-series table
--   • Enables efficient JOINs for temporal queries
--   • Maps each attribute to its value_type so Rust knows which typed column
--     to read from attribute_history (Number → value_number, etc.)
--
-- The UNIQUE constraint with NULLS NOT DISTINCT enforces NGSI-LD identity
-- rules: an entity can have multiple instances of the same attribute name
-- only if they carry distinct datasetId values (clause 4.5.5).
-- ============================================================================

CREATE TABLE temporal_entity_attributes (
    -- Surrogate key — referenced by attribute_history.tea_id
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- The entity this attribute belongs to
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,

    -- Expanded IRI of the attribute, e.g.
    -- "https://uri.etsi.org/ngsi-ld/default-context/speed"
    attribute_name TEXT NOT NULL,

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
    UNIQUE NULLS NOT DISTINCT (entity_id, attribute_name, dataset_id)
);

-- Lookup by entity + attribute (used when inserting history or resolving TEA id)
CREATE INDEX idx_tea_entity_attribute ON temporal_entity_attributes(entity_id, attribute_name);
