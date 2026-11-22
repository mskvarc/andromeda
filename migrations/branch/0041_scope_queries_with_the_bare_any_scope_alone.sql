-- ============================================================================
-- 0041: Scope queries with the bare any-scope wildcard alone
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.19
--
-- Clause 4.19 writes the bare `/#` only as a whole scope query, as
-- `ScopesQ =/ %x2F %23`. A Subscription v1.0.0 stored may hold it beside
-- other Scopes, as one alternative among others or inside a group, which a
-- scope query read now refuses. The rewrite stores the expression each such
-- `scope_q` already meant. It is `/#` where an alternative is the bare `/#`
-- or a group holds nothing else, since `/#` beside other alternatives
-- selects what it selects alone. Otherwise each group drops its bare `/#`,
-- which adds nothing to the Scopes beside it, as each of them already
-- requires a Scope, and a group left with one Scope loses its parentheses.
-- A `scope_q` is stored as the broker renders a scope query, its
-- alternatives joined by `,` and a group of two or more Scopes written as
-- `(` and `)` around Scopes joined by `;`, so the rewrite reads that form.
-- ============================================================================
-- `scope_q` with no bare `/#` beside other Scopes. Temporary, as only the
-- rewrite here calls it. Stable, as `array_to_string` is.
create function pg_temp.scope_query_without_combined_any_scope (scope_q text)
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

update
    subscriptions
set
    scope_q = pg_temp.scope_query_without_combined_any_scope(scope_q)
where
    scope_q <> '/#'
    and scope_q ~ '(^|[,(;])/#($|[,;)])';

drop function pg_temp.scope_query_without_combined_any_scope;
