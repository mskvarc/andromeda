-- ============================================================================
-- 0016: Entity maps
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.25, 5.2.39, 5.5.14, 5.14
--
-- An EntityMap pins the entities a distributed query matched, so that
-- follow-up pages, which may reach any broker instance serving the tenant,
-- consider the same entities and contact the same context sources. That
-- makes it durable tenant state rather than a per-process cache.
-- ============================================================================
create table entity_maps (
    entity_map_key bigint generated always as identity,
    created_at timestamptz not null default now(),
    expires_at timestamptz not null,
    id text compression lz4 collate "C" not null,
    -- An opaque `<operation>:<sha256 hex>` digest of the creating request's
    -- filter parameters. Clause 5.5.14 has a later request that references
    -- the map reuse those parameters, entity identifiers, pagination and the
    -- temporal query excepted; the digest is written once and compared for
    -- equality.
    request_fingerprint text compression lz4 collate "C",
    constraint pk_entity_maps primary key (entity_map_key),
    constraint uq_entity_maps_id unique (id),
    constraint chk_entity_maps_id_length check (octet_length(id) <= 1024)
);

create index idx_entity_maps_expires_at on entity_maps (expires_at);

-- One row per (entity, source) pair of Table 5.2.39-2 `entityMap`.
--
-- Every page that references a map reads the whole map in the order the
-- query produced it, so the key is that order: the read walks the key with
-- no sort. The pairs are written once, when the map is created, and the
-- broker never produces the same pair twice for one map.
create table entity_map_entries (
    entity_map_key bigint not null,
    -- The order in which the query produced the pair, unique within the map.
    position integer not null,
    entity_id text compression lz4 collate "C" not null,
    -- `@none` for an entity this broker holds, otherwise the registration
    -- whose context source returned it.
    source text compression lz4 collate "C" not null,
    constraint pk_entity_map_entries primary key (entity_map_key, position),
    constraint fk_entity_map_entries_entity_map foreign key (entity_map_key) references entity_maps (entity_map_key) on delete cascade,
    constraint chk_entity_map_entries_entity_id_length check (octet_length(entity_id) <= 1024),
    constraint chk_entity_map_entries_source_length check (octet_length(source) <= 1024)
);

-- A page that finds entities no longer matching sheds them from the map
-- (clauses 4.3.6.7 and 5.5.14), naming them by identifier.
create index idx_entity_map_entries_entity_id on entity_map_entries (entity_map_key, entity_id);

-- The EntityMap each contributing context source created for the same query
-- (Table 5.2.39-2 `linkedMaps`).
create table entity_map_links (
    entity_map_key bigint not null,
    registration_id text compression lz4 collate "C" not null,
    linked_map_id text compression lz4 collate "C" not null,
    constraint pk_entity_map_links primary key (entity_map_key, registration_id),
    constraint fk_entity_map_links_entity_map foreign key (entity_map_key) references entity_maps (entity_map_key) on delete cascade,
    constraint chk_entity_map_links_registration_id_length check (octet_length(registration_id) <= 1024),
    constraint chk_entity_map_links_linked_map_id_length check (octet_length(linked_map_id) <= 1024)
);

