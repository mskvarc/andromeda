-- ============================================================================
-- 0007: Composite types
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.5.2
--
-- The composite types the branch relations declare columns of, and the one
-- their writes bind such a column through. They live in
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
-- `object_type_keys` are `entity_type_names` keys. `proof_seals` holds the
-- clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members.
create type sub_attribute as (
    parent integer,
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
    proof_seals jsonb,
    unit_code_id smallint,
    observed_at timestamptz,
    created_at timestamptz,
    modified_at timestamptz,
    expires_at timestamptz
);

-- The sub-attributes of one attribute instance as one value. A statement that
-- writes many `current_attributes` rows binds the `sub_attributes` of every
-- row as one array parameter, and a PostgreSQL array is rectangular, so arrays
-- of differing lengths travel each wrapped in one of these. No column has this
-- type.
create type sub_attribute_tree as (
    elements sub_attribute[]
);

-- One top-level attribute instance of an Entity's state outside
-- `current_attributes`: a row of that table without its `entity_key`, field
-- for field in the table's column order and with the same types, so a record
-- and a current row decode alike and SQL moves one into the other without a
-- cast. The codes and keys are those of `sub_attribute`; `ordinal` is the
-- instance's written position among the Entity's instances, from 0.
create type attribute_instance_record as (
    created_at timestamptz,
    modified_at timestamptz,
    attribute_name_key integer,
    ordinal integer,
    attribute_kind smallint,
    value_kind smallint,
    value_date_time timestamptz,
    observed_at timestamptz,
    expires_at timestamptz,
    value_datatype_key integer,
    value_type_key integer,
    unit_code_id smallint,
    value_boolean boolean,
    dataset_id text,
    value_number numeric,
    value_string text,
    value_language text,
    value_geometry geometry,
    value_geometry_members jsonb,
    value_object jsonb,
    value_uri text,
    value_long_text text,
    value_long_uri text,
    object_type_keys integer[],
    proof_seals jsonb,
    sub_attributes sub_attribute[]
);

-- One Entity's state beside its identifier (clause 4.5.1): its types as
-- `entity_type_names` keys in written order, its Scopes as written (empty
-- when it has none), its entity-level `expiresAt` (clause 4.22), and its
-- top-level attribute instances in ordinal order.
create type entity_state_record as (
    entity_type_name_keys integer[],
    scope_paths text[],
    expires_at timestamptz,
    instances attribute_instance_record[]
);

-- One change a respond-async job still owes its subscribers (clause 5.8.6),
-- in the shape `notification_events` stores one: the fields both hold carry
-- the same names, codes and order, and a deletion's removed state is
-- `entity_snapshot` alone. Beside them the job keeps what a notification
-- built from the change needs before any event row exists: the Entity's
-- types as `entity_type_name_keys`, the instant the change occurred, and its
-- origin (1 local, 2 forwarded from a context source). It has no ordinal:
-- the replay numbers the events it records from the job's `base_ordinal` in
-- the order the job holds them.
create type pending_change_event as (
    lifecycle smallint,
    entity_id text,
    entity_type_name_keys integer[],
    change_kinds smallint[],
    change_attribute_name_keys integer[],
    change_dataset_ids text[],
    occurred_at timestamptz,
    origin smallint,
    entity_snapshot entity_state_record,
    previous_snapshot entity_state_record,
    entity_created_at timestamptz,
    entity_modified_at timestamptz,
    entity_expires_at timestamptz
);

-- One entry of a temporal append intent (clause 4.5.6), in the shape its
-- history row will be written in: the `attribute_history` columns but the
-- write identity and the stream, in the table's column order, behind the
-- three members that name the row's grain. The queue holds an entry exactly
-- as the drain will store it, so applying an intent moves its entries into
-- history without reading them back into the broker.
--
-- `entry_kind` is 1 for an attribute observation, 2 for the end of an
-- attribute instance (clauses 4.5.7 and 4.5.8: `value_kind` 0 and
-- `deleted_at` the write's instant), 3 for a Scope event (clause 4.18) and 4
-- for a Scope assertion, recorded only where the Scope in force at the write
-- differs.
-- `attribute_name_key` is an `attribute_names` key and with `dataset_id`
-- names the stream an attribute entry extends. A Scope entry has no
-- `attribute_name_key`, `dataset_id` or `attribute_kind`; its `value_kind` is
-- the `scope_history` code (0 a deletion, 1 a path), its `ltree` label path is
-- `scope_path` and the Scope as written `verbatim_scope_path`, both NULL for a
-- deletion. The other codes and keys are those of `attribute_history`.
create type temporal_entry as (
    entry_kind smallint,
    attribute_name_key integer,
    dataset_id text,
    attribute_kind smallint,
    value_kind smallint,
    observed_at timestamptz,
    created_at timestamptz,
    modified_at timestamptz,
    deleted_at timestamptz,
    value_date_time timestamptz,
    expires_at timestamptz,
    generated_instance_id uuid,
    value_type_key integer,
    unit_code_id smallint,
    value_boolean boolean,
    verbatim_instance_id text,
    value_number numeric,
    value_string text,
    value_geometry geometry,
    value_geometry_members jsonb,
    value_object jsonb,
    value_uri text,
    object_type_keys integer[],
    proof_seals jsonb,
    sub_attributes sub_attribute[],
    scope_path text,
    verbatim_scope_path text
);

-- One intent of a commit as the enqueue binds it: every intent of a commit
-- travels in one array parameter, and an intent's entries, whose count
-- differs from intent to intent, travel each intent in one value.
-- `entity_type_name_keys` are `entity_type_names` keys, empty for an intent
-- that only ends instances; `base_ordinal` is the write ordinal of the first
-- entry. `entry_needs` are the codes of the application statements' needs the
-- entries meet, so an application skips a statement no intent reaches without
-- reading `entries`.
create type temporal_intent as (
    entity_id text,
    entity_type_name_keys integer[],
    base_ordinal integer,
    entries temporal_entry[],
    entry_needs smallint[]
);

