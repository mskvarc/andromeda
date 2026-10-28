-- ============================================================================
-- 0010: JSON-LD contexts
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
create table jsonld_contexts (
    created_at timestamptz not null default now(),
    last_usage timestamptz not null default now(),
    number_of_hits bigint not null default 0,
    kind smallint not null,
    -- When a cached context must be fetched again.
    expires_at timestamptz,
    id text compression lz4 collate "C" not null,
    -- The URL the context was fetched from. Unique, so one remote context is
    -- cached once; NULL for a hosted or implicitly created context with no
    -- origin, and never NULL for a cached one.
    url text compression lz4 collate "C",
    -- The URL a cached context was finally read from, after redirects and a
    -- JSON-LD alternate link: the base its own relative references resolve
    -- against. NULL for a context the broker stores itself, which was never
    -- read from anywhere. Nothing looks a row up by it or orders by it, so it
    -- has no index, no length bound and no byte-wise collation.
    document_url text compression lz4 collate "C",
    payload json compression lz4 not null,
    constraint pk_jsonld_contexts primary key (id),
    constraint uq_jsonld_contexts_url unique (url),
    constraint chk_jsonld_contexts_kind check (kind between 1 and 3),
    constraint chk_jsonld_contexts_cached_has_url check (kind <> 1 or url is not null),
    constraint chk_jsonld_contexts_document_url_kind check ((kind = 1) = (document_url is not null)),
    constraint chk_jsonld_contexts_id_length check (octet_length(id) <= 1024),
    constraint chk_jsonld_contexts_url_length check (octet_length(url) <= 1024)
);

-- The list operation (clause 5.13.3) filters by kind.
create index idx_jsonld_contexts_kind on jsonld_contexts (kind);
