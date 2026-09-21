-- ============================================================================
-- 0022: Entity Maps
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
-- ============================================================================

CREATE TABLE entity_maps (
    -- Surrogate the child tables reference; never rendered.
    map_key BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    -- System-managed creation instant.
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When the map stops being usable (clause 5.2.39 `expiresAt`).
    expires_at TIMESTAMPTZ NOT NULL,

    -- EntityMap URI, e.g. "urn:ngsi-ld:EntityMap:<uuid>".
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    -- Clause 5.5.14: a subsequent request referencing an EntityMap shall reuse
    -- the creating request's parameters (bar entity ids, pagination and, for
    -- temporal requests, the temporal query). This opaque `<operation>:<sha256
    -- hex>` digest of those filter parameters is written once at creation and
    -- compared for equality only, hence `COLLATE "C"`.
    request_fingerprint TEXT COMPRESSION lz4 COLLATE "C"
);

-- The expiry sweep reads maps due at or before its cutoff.
CREATE INDEX idx_entity_maps_expires_at ON entity_maps (expires_at);

CREATE TABLE entity_map_entries (
    -- Owning map.
    map_key BIGINT NOT NULL REFERENCES entity_maps(map_key) ON DELETE CASCADE,

    -- Order the query produced the (entity, source) pair in.
    position INTEGER NOT NULL,

    -- The matched entity.
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- '@none' for an entity this broker holds, else the registration id.
    source TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    PRIMARY KEY (map_key, entity_id, source)
);

CREATE TABLE entity_map_links (
    -- Owning map.
    map_key BIGINT NOT NULL REFERENCES entity_maps(map_key) ON DELETE CASCADE,

    -- The registration whose context source contributed.
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- The EntityMap that context source created for the same query.
    linked_map_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    PRIMARY KEY (map_key, registration_id)
);
