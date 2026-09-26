-- ============================================================================
-- 0011: JSON-LD contexts
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.13, 6.3.16
--
-- The `@context` documents the broker expands and compacts with: contexts
-- fetched from a remote URL and cached, contexts hosted through the API, and
-- contexts the broker creates implicitly.
--
-- Codes:
--
--   kind: 1 Cached, 2 Hosted, 3 ImplicitlyCreated
--
-- A remote context whose URL exceeds the identifier bound is served without
-- being cached.
-- ============================================================================

CREATE TABLE jsonld_contexts (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_usage TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    number_of_hits BIGINT NOT NULL DEFAULT 0,
    kind SMALLINT NOT NULL,
    -- When a cached context must be fetched again.
    expires_at TIMESTAMPTZ,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- The URL the context was fetched from. Unique, so one remote context is
    -- cached once; NULL for a hosted or implicitly created context with no
    -- origin, and never NULL for a cached one.
    url TEXT COMPRESSION lz4 COLLATE "C",
    payload JSONB COMPRESSION lz4 NOT NULL,
    extra_info JSONB COMPRESSION lz4,
    CONSTRAINT pk_jsonld_contexts PRIMARY KEY (id),
    CONSTRAINT uq_jsonld_contexts_url UNIQUE (url),
    CONSTRAINT chk_jsonld_contexts_kind CHECK (kind BETWEEN 1 AND 3),
    CONSTRAINT chk_jsonld_contexts_cached_has_url CHECK (kind <> 1 OR url IS NOT NULL),
    CONSTRAINT chk_jsonld_contexts_id_length CHECK (octet_length(id) <= 1024),
    CONSTRAINT chk_jsonld_contexts_url_length CHECK (octet_length(url) <= 1024)
);

-- The list operation (clause 5.13.3) filters by kind.
CREATE INDEX idx_jsonld_contexts_kind ON jsonld_contexts(kind);
