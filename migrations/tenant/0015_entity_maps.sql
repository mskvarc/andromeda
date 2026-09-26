-- ============================================================================
-- 0015: Entity maps
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.25, 5.2.39, 5.5.14, 5.14
--
-- An EntityMap pins the entities a distributed query matched, so that
-- follow-up pages, which may reach any broker instance serving the tenant,
-- consider the same entities and contact the same context sources. That
-- makes it durable tenant state rather than a per-process cache.
-- ============================================================================

CREATE TABLE entity_maps (
    entity_map_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- An opaque `<operation>:<sha256 hex>` digest of the creating request's
    -- filter parameters. Clause 5.5.14 has a later request that references
    -- the map reuse those parameters, entity identifiers, pagination and the
    -- temporal query excepted; the digest is written once and compared for
    -- equality.
    request_fingerprint TEXT COMPRESSION lz4 COLLATE "C",
    CONSTRAINT pk_entity_maps PRIMARY KEY (entity_map_key),
    CONSTRAINT uq_entity_maps_id UNIQUE (id),
    CONSTRAINT chk_entity_maps_id_length CHECK (octet_length(id) <= 1024)
);

CREATE INDEX idx_entity_maps_expires_at ON entity_maps (expires_at);

-- One row per (entity, source) pair of Table 5.2.39-2 `entityMap`.
CREATE TABLE entity_map_entries (
    entity_map_key BIGINT NOT NULL,
    -- The order in which the query produced the pair.
    position INTEGER NOT NULL,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- `@none` for an entity this broker holds, otherwise the registration
    -- whose context source returned it.
    source TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_entity_map_entries PRIMARY KEY (entity_map_key, entity_id, source),
    CONSTRAINT fk_entity_map_entries_entity_map FOREIGN KEY (entity_map_key)
        REFERENCES entity_maps(entity_map_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_map_entries_entity_id_length CHECK (octet_length(entity_id) <= 1024),
    CONSTRAINT chk_entity_map_entries_source_length CHECK (octet_length(source) <= 1024)
);

-- The EntityMap each contributing context source created for the same query
-- (Table 5.2.39-2 `linkedMaps`).
CREATE TABLE entity_map_links (
    entity_map_key BIGINT NOT NULL,
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    linked_map_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_entity_map_links PRIMARY KEY (entity_map_key, registration_id),
    CONSTRAINT fk_entity_map_links_entity_map FOREIGN KEY (entity_map_key)
        REFERENCES entity_maps(entity_map_key) ON DELETE CASCADE,
    CONSTRAINT chk_entity_map_links_registration_id_length CHECK (octet_length(registration_id) <= 1024),
    CONSTRAINT chk_entity_map_links_linked_map_id_length CHECK (octet_length(linked_map_id) <= 1024)
);
