CREATE TABLE jsonld_contexts (
    -- The Local Identifier (e.g., "urn:ngsi-ld:Context:550e8400-e29b...")
    -- This is the PRIMARY identifier for API operations.
    id TEXT PRIMARY KEY,

    -- The Original URL.
    -- Nullable for 'Hosted' or 'ImplicitlyCreated' if they don't have an origin.
    -- Unique constraint ensures we don't double-cache the same URL.
    url TEXT UNIQUE,

    -- "Cached", "Hosted", or "ImplicitlyCreated"
    kind TEXT NOT NULL,

    -- The actual JSON-LD payload
    payload JSONB NOT NULL,

    -- Metadata required by Spec
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_usage TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    number_of_hits INTEGER NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ, -- Optional: for cache invalidation logic

    -- Optional: Custom extra info (e.g., headers from the source)
    extra_info JSONB
);

-- Index for filtering by kind (List operation)
CREATE INDEX idx_jsonld_contexts_kind ON jsonld_contexts(kind);
