-- ============================================================================
-- 0045: Subscription queries with colon names quoted
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.6.2, 4.9 and 5.5.7
--
-- An unquoted name of a `q` or a `csf` is the clause 4.9 `AttrName`, and any
-- other name is written as a `quotedStr` whose value is the name. v1.0.0
-- stored a Subscription's `q`, its written members and its `csf` in the
-- expanded spelling, every name an absolute IRI, and wrote one unquoted when
-- it held no dot, which a path read now refuses. This file rewrites the `q`,
-- the `q_written_members` and the `csf` of each Subscription of the branch it
-- runs in, the live one or a Snapshot's, entity and Context Source
-- Registration Subscriptions alike, with the function of the tenant's
-- `query_name_rewrite` schema, which wraps each unquoted name holding a colon
-- in double quotes and changes nothing else. A Subscription holding no such
-- name is kept as written.
--
-- The rewrite runs in one block, which ends at once when the branch holds no
-- Subscription, and names nothing outside the branch before that test. The
-- tenant set drops the `query_name_rewrite` schema once every branch has
-- taken this migration, and a Snapshot branch created after that runs this
-- file over a `subscriptions` that is still empty, so it never names a
-- dropped function.
-- ============================================================================
do $$
begin
    if not exists (
        select
        from
            subscriptions) then
        return;
    end if;
    update
        subscriptions
    set
        q = query_name_rewrite.path_text_with_colon_names_quoted(q),
        q_written_members = query_name_rewrite.path_text_with_colon_names_quoted(q_written_members),
        csf = query_name_rewrite.path_text_with_colon_names_quoted(csf)
    where
        query_name_rewrite.path_text_with_colon_names_quoted(q) is distinct from q
        or query_name_rewrite.path_text_with_colon_names_quoted(q_written_members) is distinct from q_written_members
        or query_name_rewrite.path_text_with_colon_names_quoted(csf) is distinct from csf;
end
$$;
