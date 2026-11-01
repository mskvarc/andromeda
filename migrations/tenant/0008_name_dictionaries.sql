-- ============================================================================
-- 0008: Name dictionaries
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.17
--
-- The dictionaries that intern expanded attribute IRIs and expanded entity
-- type IRIs. Every relation that names an attribute or an entity type stores
-- the dictionary key instead of the IRI, and a set of them as an array of
-- keys, so a clause 4.17 type selection is served by a GIN index over
-- four-byte keys rather than over IRIs.
--
-- Keys are `INTEGER`: attribute and type IRIs are open-world values the
-- specification does not limit, so a 32,767-key ceiling would be an accident
-- rather than a modelled constraint.
--
-- Neither dictionary ever deletes or re-keys a row, so a key, once minted,
-- names the same IRI for the life of the tenant. Triggers enforce it: a row
-- trigger refuses every update and delete, and a statement trigger refuses a
-- truncate, so an insert is the only change either dictionary admits.
-- ============================================================================
create table attribute_names (
    attribute_name_key integer generated always as identity (cache 32),
    name text compression lz4 collate "C" not null,
    constraint pk_attribute_names primary key (attribute_name_key),
    constraint uq_attribute_names_name unique (name),
    constraint chk_attribute_names_name_length check (char_length(name) >= 1 and octet_length(name) <= 1024)
);

create table entity_type_names (
    entity_type_name_key integer generated always as identity (cache 32),
    name text compression lz4 collate "C" not null,
    constraint pk_entity_type_names primary key (entity_type_name_key),
    constraint uq_entity_type_names_name unique (name),
    constraint chk_entity_type_names_name_length check (char_length(name) >= 1 and octet_length(name) <= 1024)
);

create trigger trg_attribute_names_append_only_row
    before update or delete on attribute_names for each row
    execute function refuse_dictionary_change ();

create trigger trg_attribute_names_append_only_truncate
    before truncate on attribute_names for each statement
    execute function refuse_dictionary_change ();

create trigger trg_entity_type_names_append_only_row
    before update or delete on entity_type_names for each row
    execute function refuse_dictionary_change ();

create trigger trg_entity_type_names_append_only_truncate
    before truncate on entity_type_names for each statement
    execute function refuse_dictionary_change ();
