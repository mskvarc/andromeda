-- ============================================================================
-- 0015: Entity Maps
-- ============================================================================
-- NGSI-LD EntityMap (clauses 4.5.25, 5.2.39, 5.5.14, 5.14). An EntityMap pins
-- the entities a distributed query matched so follow-up pages, which may reach
-- any instance serving the tenant, consider the same entities and contact the
-- same context sources. It is therefore durable, tenant-scoped state rather
-- than a per-process cache.
--
-- `entity_maps` is the control record: identifier and expiry. `entity_map_entries`
-- holds one row per (entity, source): the source is the `@none` token for an
-- entity this broker holds, or the registration whose context source returned
-- it (Table 5.2.39-2 `entityMap`). `entity_map_links` holds the EntityMap each
-- contributing context source created for the same query (`linkedMaps`).
--
-- Physical shape follows the snapshot tables: a database-issued surrogate keys
-- the children, identifiers are `COLLATE "C"` and `COMPRESSION lz4`, and
-- `position` preserves the order the query produced.
--
-- Every identifier column is bounded at 1024 UTF-8 octets, the identifier
-- bound (`IDENTIFIER_BYTE_LIMIT` in `andromeda-ngsi-ld-lexicon`) each is parsed
-- under. The bound is chosen so that two identifiers fit one btree entry: the
-- primary key of `entity_map_entries` carries an entity id and a source, and a
-- btree refuses an entry past 2704 bytes.
-- ============================================================================

CREATE TABLE entity_maps (
    -- Surrogate the child tables reference; never rendered.
    map_key BIGINT GENERATED ALWAYS AS IDENTITY,

    -- System-managed creation instant.
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When the map stops being usable (clause 5.2.39 `expiresAt`).
    expires_at TIMESTAMPTZ NOT NULL,

    -- EntityMap URI, e.g. "urn:ngsi-ld:EntityMap:<uuid>".
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- Clause 5.5.14: a subsequent request referencing an EntityMap shall reuse
    -- the creating request's parameters (bar entity ids, pagination and, for
    -- temporal requests, the temporal query). This opaque `<operation>:<sha256
    -- hex>` digest of those filter parameters is written once at creation and
    -- compared for equality only, hence `COLLATE "C"`.
    request_fingerprint TEXT COMPRESSION lz4 COLLATE "C",

    CONSTRAINT pk_entity_maps PRIMARY KEY (map_key),
    CONSTRAINT uq_entity_maps_id UNIQUE (id),
    CONSTRAINT chk_entity_maps_id_length CHECK (octet_length(id) <= 1024)
);

-- The expiry sweep reads maps due at or before its cutoff.
CREATE INDEX idx_entity_maps_expires_at ON entity_maps (expires_at);

CREATE TABLE entity_map_entries (
    -- Owning map.
    map_key BIGINT NOT NULL,

    -- Order the query produced the (entity, source) pair in.
    position INTEGER NOT NULL,

    -- The matched entity.
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- '@none' for an entity this broker holds, else the registration id.
    source TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    CONSTRAINT pk_entity_map_entries PRIMARY KEY (map_key, entity_id, source),
    CONSTRAINT fk_entity_map_entries_map FOREIGN KEY (map_key)
        REFERENCES entity_maps(map_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_map_entries_entity_id_length CHECK (octet_length(entity_id) <= 1024),
    CONSTRAINT chk_entity_map_entries_source_length CHECK (octet_length(source) <= 1024)
);

CREATE TABLE entity_map_links (
    -- Owning map.
    map_key BIGINT NOT NULL,

    -- The registration whose context source contributed.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The EntityMap that context source created for the same query.
    linked_map_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    CONSTRAINT pk_entity_map_links PRIMARY KEY (map_key, registration_id),
    CONSTRAINT fk_entity_map_links_map FOREIGN KEY (map_key)
        REFERENCES entity_maps(map_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_map_links_registration_id_length CHECK (octet_length(registration_id) <= 1024),
    CONSTRAINT chk_entity_map_links_linked_map_id_length CHECK (octet_length(linked_map_id) <= 1024)
);
