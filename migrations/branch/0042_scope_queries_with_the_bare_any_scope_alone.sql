-- ============================================================================
-- 0042: Scope queries with the bare any-scope wildcard alone
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.19
--
-- Rewrites the `scope_q` of each Subscription of the branch it runs in, the
-- live one or a Snapshot's, that holds the bare `/#` beside other Scopes,
-- with the functions of the tenant's `scope_query_rewrite` schema. The
-- tenant set's migration before this one says what the rewrite stores. A
-- `scope_q` holding no such `/#` is kept as written.
--
-- The rewrite runs in one block, which ends at once when the branch holds no
-- Subscription, and names nothing outside the branch before that test. The
-- tenant set drops the `scope_query_rewrite` schema once every branch has
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
        scope_q = scope_query_rewrite.scope_query_without_combined_any_scope(scope_q)
    where
        scope_query_rewrite.holds_combined_any_scope(scope_q);
end
$$;
