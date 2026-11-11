-- ============================================================================
-- 0035: Posted queries
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.5.9, 6.3.10, 6.23.3.1, 6.24.3.1
--
-- A query sent by POST travels in the request body, so a page link, which a
-- client follows with GET, cannot carry it. The broker holds the query of a
-- paged POST here, and each page link names it by its digest. A query is held
-- for a configured lifetime from the last POST that sent it; the expiry sweep
-- deletes it after, and a per-tenant quota deletes the ones expiring soonest
-- first.
-- ============================================================================
create table posted_queries (
    -- 1 for Query Entities, 2 for Query Temporal Evolution of Entities.
    kind smallint not null,
    expires_at timestamptz not null,
    -- The digest of the query, its API and its @context, in the unpadded
    -- base64url spelling a page link carries.
    digest text compression lz4 collate "C" not null,
    -- The clause 5.2.23 Query as the broker read it, without its @context.
    query json compression lz4 not null,
    -- The context the POST named in its Link header, if it named one there.
    context_link text compression lz4 collate "C",
    -- The @context member of the POST body, if it named one there.
    context_body json compression lz4,
    constraint pk_posted_queries primary key (digest),
    constraint chk_posted_queries_kind check (kind between 1 and 2),
    constraint chk_posted_queries_one_context check (num_nonnulls(context_link, context_body) <= 1),
    constraint chk_posted_queries_digest_length check (octet_length(digest) <= 22)
);

-- The expiry sweep and the quota read the queries in expiry order.
create index idx_posted_queries_expires_at on posted_queries (expires_at);
