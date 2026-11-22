-- ============================================================================
-- 0041: Snapshot queries with the bare any-scope wildcard alone
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.19
--
-- Clause 4.19 writes the bare `/#` only as a whole scope query, as
-- `ScopesQ =/ %x2F %23`. A Snapshot or a Subscription v1.0.0 stored may hold
-- it beside other Scopes, as one alternative among others or inside a group,
-- which a scope query read now refuses. The rewrite stores the expression
-- each such `scopeQ` already meant. It is `/#` where an alternative is the
-- bare `/#` or a group holds nothing else, since `/#` beside other
-- alternatives selects what it selects alone. Otherwise each group drops its
-- bare `/#`, which adds nothing to the Scopes beside it, as each of them
-- already requires a Scope, and a group left with one Scope loses its
-- parentheses. A `scopeQ` is stored as the broker renders a scope query, its
-- alternatives joined by `,` and a group of two or more Scopes written as `(`
-- and `)` around Scopes joined by `;`, so the rewrite reads that form.
--
-- This file rewrites the `scopeQ` of each query `snapshot_queries` and
-- `snapshot_temporal_queries` hold, the clause 5.2.23 Queries a Snapshot was
-- captured by, which v1.0.0 stored as a JSON array or NULL. A query holding
-- no such `scopeQ` is kept as written. The branch set then rewrites the
-- `scope_q` of the Subscriptions in every branch, the live one and each
-- Snapshot's, with the functions of schema `scope_query_rewrite`, and the
-- tenant set drops the schema once every branch has taken that migration.
-- ============================================================================
-- The functions the rewrite of this set and of the branch set share.
create schema scope_query_rewrite;

-- Whether `scope_q` holds the bare `/#` beside another Scope.
create function scope_query_rewrite.holds_combined_any_scope (scope_q text)
    returns boolean
    language sql
    immutable
    parallel safe
    return scope_q <> '/#'
    and scope_q ~ '(^|[,(;])/#($|[,;)])';

-- `scope_q` with no bare `/#` beside other Scopes. Stable, as
-- `array_to_string` is.
create function scope_query_rewrite.scope_query_without_combined_any_scope (scope_q text)
    returns text
    language sql
    stable
    parallel safe
    return (
        select
            case when bool_or(cardinality(alternative.scopes) = 0) then
                '/#'
            else
                string_agg(case cardinality(alternative.scopes) when 1 then
                        alternative.scopes[1]
                    else
                        '(' || array_to_string(alternative.scopes, ';') || ')'
                    end, ',' order by alternative.ordinal)
            end
        from (
            select
                written.ordinal,
                array_remove(string_to_array(btrim(written.alternative, '()'), ';'), '/#') as scopes
            from
                unnest(string_to_array(scope_q, ',')) with ordinality as written (alternative, ordinal)) as alternative);

-- Whether one of the queries `queries` holds a `scopeQ` with the bare `/#`
-- beside another Scope. Temporary, as only this file calls it.
create function pg_temp.queries_hold_combined_any_scope (queries json)
    returns boolean
    language sql
    immutable
    parallel safe
    return exists (
        select
        from
            json_array_elements(queries) as written (query)
        where
            scope_query_rewrite.holds_combined_any_scope(written.query ->> 'scopeQ'));

-- The queries `queries` in their order, each `scopeQ` holding the bare `/#`
-- beside another Scope rewritten and every other query as written.
-- Temporary, as only this file calls it.
create function pg_temp.queries_without_combined_any_scope (queries json)
    returns json
    language sql
    stable
    parallel safe
    return (
        select
            json_agg(case when scope_query_rewrite.holds_combined_any_scope(written.query ->> 'scopeQ') then
                    jsonb_set(written.query::jsonb, '{scopeQ}', to_jsonb(scope_query_rewrite.scope_query_without_combined_any_scope(written.query ->> 'scopeQ')))::json
                else
                    written.query
                end order by written.ordinal)
        from
            json_array_elements(queries) with ordinality as written (query, ordinal));

update
    snapshots
set
    snapshot_queries = pg_temp.queries_without_combined_any_scope(snapshot_queries)
where
    pg_temp.queries_hold_combined_any_scope(snapshot_queries);

update
    snapshots
set
    snapshot_temporal_queries = pg_temp.queries_without_combined_any_scope(snapshot_temporal_queries)
where
    pg_temp.queries_hold_combined_any_scope(snapshot_temporal_queries);

drop function pg_temp.queries_without_combined_any_scope, pg_temp.queries_hold_combined_any_scope;
