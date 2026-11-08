-- ============================================================================
-- 0011: Registration information
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.17, 5.2.8, 5.2.9, 5.2.10, 5.10, 5.12
--
-- A registration's `information` (clause 5.2.10), stored once and lossless:
-- one row per `EntityInfo` of an item, or one row without entity columns for
-- an item that has no `entities`. Every response body is assembled from these
-- rows, and discovery (clause 5.10) and the forwarding coverage decision
-- (clause 5.12) read the same rows. Clause 5.12 matches an entity
-- specification and an attribute list against "the same" RegistrationInfo,
-- so each row carries its item's names, and one row decides the type, the
-- identifier and both attribute axes together.
--
-- Absence is NULL. The indexes spell the absent axis as a sentinel the
-- readers name beside the requested value: `{*}` for the types and `''` for
-- the identifier. An item that declares no attribute name at all has NULL on
-- both name axes, which a partial index finds directly; a row whose item
-- declares names on one axis only reads the other as `{}`, which overlaps
-- nothing.
--
-- The registration's `operations` are not here: no preselect filters on them.
-- ============================================================================
create table registration_information (
    item_ordinal integer not null,
    -- 0 for an item without `entities`.
    selector_ordinal integer not null,
    -- Table 5.2.8-1 `type` as written: 1 one name, 2 an array of names.
    entity_type_shape smallint,
    registration_id text compression lz4 collate "C" not null,
    entity_types text[] compression lz4 collate "C",
    entity_id text compression lz4 collate "C",
    -- Compared as written and never evaluated: PostgreSQL's regular
    -- expression dialect is not the one clause 5.2.8 mandates.
    entity_id_pattern text compression lz4 collate "C",
    -- The restatement of `entity_id_pattern` the broker renders for this
    -- engine: every embedded option applied and every class written out, so
    -- `~` reads the pattern as the broker's in-memory engine does (clause 4.9;
    -- PostgreSQL's own `(?i)` folds only ASCII). The broker writes it beside
    -- the pattern; nothing here derives it.
    entity_id_pattern_form text compression lz4 collate "C",
    -- `attribute_names` keys, in written order, repeated on every row of the
    -- item.
    property_name_keys integer[] compression lz4,
    relationship_name_keys integer[] compression lz4,
    constraint pk_registration_information primary key (registration_id, item_ordinal, selector_ordinal),
    constraint fk_registration_information_registration foreign key (registration_id) references context_source_registrations (id) on delete cascade,
    constraint chk_registration_information_registration_id_length check (octet_length(registration_id) <= 1024),
    constraint chk_registration_information_entity_id_length check (octet_length(entity_id) <= 1024),
    constraint chk_registration_information_entity_id_pattern_length check (octet_length(entity_id_pattern) <= 1024),
    constraint chk_registration_information_item_ordinal_non_negative check (item_ordinal >= 0),
    constraint chk_registration_information_selector_ordinal_non_negative check (selector_ordinal >= 0),
    constraint chk_registration_information_entity_type_shape check ((entity_types is null) = (entity_type_shape is null) and entity_type_shape between 1 and 2 and (entity_type_shape = 2 or cardinality(entity_types) = 1) and cardinality(entity_types) > 0 and array_position(entity_types, null) is null),
    constraint chk_registration_information_selector check (entity_types is not null or (entity_id is null and entity_id_pattern is null and selector_ordinal = 0)),
    constraint chk_registration_information_entity_id_pattern_form check ((entity_id_pattern is null) = (entity_id_pattern_form is null)),
    constraint chk_registration_information_name_keys check (cardinality(property_name_keys) > 0 and array_position(property_name_keys, null) is null and 0 < all (property_name_keys) and cardinality(relationship_name_keys) > 0 and array_position(relationship_name_keys, null) is null and 0 < all (relationship_name_keys)),
    constraint chk_registration_information_item_presence check (num_nonnulls(entity_types, property_name_keys, relationship_name_keys) > 0)
)
with (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200
);

create index idx_registration_information_entity_types on registration_information using gin ((coalesce(entity_types, '{*}')));

create index idx_registration_information_property_name_keys on registration_information using gin (property_name_keys);

create index idx_registration_information_relationship_name_keys on registration_information using gin (relationship_name_keys);

-- The branch of the attribute test that admits an item declaring no name,
-- which matches every attribute list (clause 5.12).
create index idx_registration_information_undeclared_names on registration_information (registration_id)
where
    property_name_keys is null and relationship_name_keys is null;

-- Not partial: the sentinel `''` is one of the two values every identifier
-- test names.
create index idx_registration_information_entity_id on registration_information ((coalesce(entity_id, '')));

-- The pattern-presence branch of the identifier test.
create index idx_registration_information_entity_id_pattern on registration_information (entity_id_pattern)
where
    entity_id_pattern is not null;

-- A queried `idPattern` matched against the registered `id` (clause 5.12),
-- which the B-tree cannot serve because the portable pattern is unanchored.
-- The trigram column is collated under the database's default, the
-- collation `pg_trgm` extracts an indexed string's trigrams under. The
-- trigrams of a pattern are taken under the collation the pattern is tested
-- under, and under `"C"` they leave out every letter outside ASCII the
-- default classifies, so the index would drop strings the pattern matches.
create index idx_registration_information_entity_id_trgm on registration_information using gin (entity_id collate "default" gin_trgm_ops);
