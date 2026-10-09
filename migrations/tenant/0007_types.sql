-- ============================================================================
-- 0007: Composite types
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.5.2
--
-- The composite types the branch relations declare columns of. They live in
-- `public`, and a Snapshot schema reaches them through its search path, so one
-- definition serves every branch. One definition is also what lets a Snapshot
-- clone copy a column of such a type between two schemas: two composite types
-- of the same shape in two schemas do not cast to each other.
--
-- Codes:
--
--   sub_attribute.attribute_kind: 1 Property, 2 Relationship,
--     3 GeoProperty, 4 VocabProperty, 5 LanguageProperty, 6 JsonProperty,
--     7 ListProperty, 8 ListRelationship
--   sub_attribute.value_kind: 1 number, 2 string, 3 boolean,
--     4 geometry, 5 object, 6 date-time, 7 uri
-- ============================================================================
-- One sub-attribute of an attribute instance (clause 4.5.2), an element of
-- the instance's `sub_attributes` array. The array lists an instance's
-- sub-attributes in written order, every parent before its children, and the
-- datasets of one sub-attribute in written order among themselves. An element
-- carries the members of an attribute instance, so every attribute family is
-- representable at every depth.
--
-- `parent` is 0 for a sub-attribute of the instance itself and otherwise the
-- one-based array position of the element it belongs to. The string and URI
-- fields carry no length bound and no overflow field: no B-tree keys an
-- element's value, so nothing limits its length. `attribute_name_key` is an
-- `attribute_names` key; `value_datatype_key`, `value_type_key` and
-- `object_type_keys` are `entity_type_names` keys.
create type sub_attribute as (
    parent smallint,
    attribute_name_key integer,
    dataset_id text,
    attribute_kind smallint,
    value_kind smallint,
    value_number numeric,
    value_string text,
    value_boolean boolean,
    value_date_time timestamptz,
    value_geometry geometry,
    value_geometry_members jsonb,
    value_object jsonb,
    value_uri text,
    value_datatype_key integer,
    value_language text,
    value_type_key integer,
    object_type_keys integer[],
    unit_code_id smallint,
    observed_at timestamptz,
    created_at timestamptz,
    modified_at timestamptz,
    expires_at timestamptz
);

