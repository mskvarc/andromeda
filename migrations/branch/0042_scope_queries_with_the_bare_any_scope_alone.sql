-- ============================================================================
-- 0042: Scope queries with the bare any-scope wildcard alone
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.19
--
-- Clause 4.19 writes the bare `/#` only as a whole scope query, as
-- `ScopesQ =/ %x2F %23`. A Subscription v1.0.0 stored may hold it beside
-- other Scopes, as one alternative among others or inside a group, which a
-- scope query read now refuses. This file rewrites the `scope_q` of each such
-- Subscription of the branch it runs in, the live one or a Snapshot's, with
-- the functions of the tenant's `scope_query_rewrite` schema, to the
-- expression it already meant. That is `/#` where an alternative is the bare
-- `/#` or a group holds nothing else, since `/#` beside other alternatives
-- selects what it selects alone. Otherwise each group drops its bare `/#`,
-- which adds nothing to the Scopes beside it, as each of them already
-- requires a Scope, and a group left with one Scope loses its parentheses. A
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
