-- ============================================================================
-- 0039: Entity map request inputs
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.5.9.1 and 5.5.14
--
-- A request under an Entity Map is held to every input of the request that
-- created the map, its `@context` among them, so each map records the digest
-- of those inputs beside the digest of the query it selects. A map v1.0.0
-- stored records only a digest of its filters as written, which compares with
-- no follow-up, so every stored map is removed, its entries and links with
-- it. A request naming a removed map is answered as one naming a map that
-- expired: clause 5.5.14 draws no inference from it, and a read creates a new
-- map.
--
-- Removing every map leaves the three relations empty, so they are dropped
-- and created again, with the operation that created a map and its two
-- digests in columns of their own, each digest as its 32 bytes.
--
-- Codes:
--
--   request_kind: 1 query-entities, 2 retrieve-entity, 3 query-temporal,
--     4 retrieve-temporal
-- ============================================================================
drop table entity_map_links, entity_map_entries, entity_maps;

-- A map records the operation and both digests of the request that created
-- it, or none of them.
create table entity_maps (
    entity_map_key bigint generated always as identity,
    expires_at timestamptz not null,
    request_kind smallint,
    id text compression lz4 collate "C" not null,
    -- The SHA-256 digest of the canonical identity of the creating request's
    -- compiled query.
    query_digest bytea compression lz4,
    -- The SHA-256 digest of every other input of the creating request that
    -- clause 5.5.14 holds a follow-up to.
    inputs_digest bytea compression lz4,
    constraint pk_entity_maps primary key (entity_map_key),
    constraint uq_entity_maps_id unique (id),
    constraint chk_entity_maps_request_kind check (request_kind between 1 and 4),
    constraint chk_entity_maps_request_record check (num_nonnulls(request_kind, query_digest, inputs_digest) in (0, 3)),
    constraint chk_entity_maps_id_length check (octet_length(id) <= 1024),
    constraint chk_entity_maps_query_digest_length check (octet_length(query_digest) = 32),
    constraint chk_entity_maps_inputs_digest_length check (octet_length(inputs_digest) = 32)
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
    constraint chk_entity_map_entries_position_non_negative check (position >= 0),
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
