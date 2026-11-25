-- ============================================================================
-- 0044: Snapshot queries with colon names quoted
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.6.2, 4.9 and 5.5.7
--
-- An unquoted name of a `q`, a `csf` or an `orderBy` term is the clause 4.9
-- `AttrName`, a letter followed by letters, digits and `_`, and any other
-- name, a prefixed name (clause 4.6.2) or an absolute IRI, is written as a
-- `quotedStr` whose value is the name. v1.0.0 read an unquoted word through
-- `:`, `/`, `-` and `#` and wrote a name quoted only when it held a dot, a
-- space or a quote, so a stored path may hold an unquoted name with a colon,
-- which a path read now refuses. The rewrite wraps each such name in double
-- quotes and changes nothing else, so it needs no `@context`.
--
-- The rewrite reads the text v1.0.0 wrote. A double-quoted token runs to the
-- next `"` that an even number of backslashes precedes and is copied as it
-- is. The scan starts where a name may stand. A comparison operator moves it
-- to where an operand stands, where everything is copied as it is, and `;`,
-- `|`, `(` or `)` moves it back. Where a name may stand, a word is the
-- longest run of characters other than `"`, whitespace and
-- `. [ ] { } ( ) ; | , = ! < > ~`. A word holding no colon is copied as it
-- is. A word holding a colon that follows a comma is the last type of a type
-- list glued to the name after it, which v1.0.0 read as the list's end, and
-- is copied as it is. A word that begins with a colon follows a quoted last
-- type, so its colon is copied and the rest is quoted when it holds a colon.
-- Any other word holding a colon is a name and is wrapped in double quotes.
-- The rewrite applied twice gives what it gives once.
--
-- This file rewrites the `q`, the `csf` and each `orderBy` term of each query
-- `snapshot_queries` and `snapshot_temporal_queries` hold, the clause 5.2.23
-- Queries a Snapshot was captured by, which v1.0.0 stored as a JSON array or
-- NULL in the spelling their creator wrote. A Snapshot whose queries hold no
-- such name is kept as written. The branch set then rewrites the `q`, the
-- `q_written_members` and the `csf` of the Subscriptions in every branch, the
-- live one and each Snapshot's, with the function of schema
-- `query_name_rewrite`, and the tenant set drops the schema once every branch
-- has taken that migration.
-- ============================================================================
-- The function the rewrite of this set and of the branch set share.
create schema query_name_rewrite;

-- `path_text`, a `q`, a `csf` or an `orderBy` term as v1.0.0 wrote it, with
-- each unquoted name holding a colon in double quotes. PL/pgSQL, as each step
-- of the scan depends on where the step before it ended. The backslash is
-- `chr(92)`, which reads alike whatever `standard_conforming_strings` says.
create function query_name_rewrite.path_text_with_colon_names_quoted (path_text text)
    returns text
    language plpgsql
    immutable
    strict
    parallel safe
    as $$
declare
    rewritten text := '';
    at_index integer := 1;
    text_length integer := length(path_text);
    mark text;
    token_end integer;
    backslashes integer;
    word text;
    operator_spelling text;
    reads_operand boolean := false;
    previous_mark text := '';
begin
    while at_index <= text_length loop
        mark := substr(path_text, at_index, 1);
        if mark = '"' then
            token_end := at_index + 1;
            while token_end <= text_length loop
                if substr(path_text, token_end, 1) = '"' then
                    backslashes := 0;
                    while token_end - backslashes - 1 > at_index
                        and substr(path_text, token_end - backslashes - 1, 1) = chr(92) loop
                        backslashes := backslashes + 1;
                    end loop;
                    exit when backslashes % 2 = 0;
                end if;
                token_end := token_end + 1;
            end loop;
            rewritten := rewritten || substr(path_text, at_index, token_end - at_index + 1);
            at_index := token_end + 1;
            previous_mark := '"';
        elsif reads_operand then
            if mark in (';', '|', '(', ')') then
                reads_operand := false;
                previous_mark := mark;
            end if;
            rewritten := rewritten || mark;
            at_index := at_index + 1;
        elsif mark in (' ', chr(9), chr(10), chr(13)) then
            rewritten := rewritten || mark;
            at_index := at_index + 1;
        elsif strpos('.[]{}();|,=!<>~', mark) > 0 then
            operator_spelling := case when substr(path_text, at_index, 3) = '!~=' then
                '!~='
            when substr(path_text, at_index, 2) in ('==', '!=', '>=', '<=', '~=') then
                substr(path_text, at_index, 2)
            when mark in ('>', '<') then
                mark
            end;
            if operator_spelling is null then
                rewritten := rewritten || mark;
                at_index := at_index + 1;
                previous_mark := mark;
            else
                rewritten := rewritten || operator_spelling;
                at_index := at_index + length(operator_spelling);
                reads_operand := true;
            end if;
        else
            token_end := at_index;
            while token_end <= text_length
                and strpos('.[]{}();|,=!<>~" ' || chr(9) || chr(10) || chr(13), substr(path_text, token_end, 1)) = 0 loop
                token_end := token_end + 1;
            end loop;
            word := substr(path_text, at_index, token_end - at_index);
            rewritten := rewritten || case when strpos(word, ':') = 0
                or previous_mark = ',' then
                word
            when left(word, 1) = ':'
                and strpos(substr(word, 2), ':') = 0 then
                word
            when left(word, 1) = ':' then
                ':"' || substr(word, 2) || '"'
            else
                '"' || word || '"'
            end;
            at_index := token_end;
            previous_mark := 'w';
        end if;
    end loop;
    return rewritten;
end
$$;

-- The query `query` with its `q`, its `csf` and each `orderBy` term
-- rewritten, and every other member as written. Stable, as `to_jsonb` is.
-- Temporary, as only this file calls it.
create function pg_temp.query_with_colon_names_quoted (query jsonb)
    returns jsonb
    language sql
    stable
    parallel safe
    return (
        select
            case when jsonb_typeof(with_csf.query #> '{ordering,orderBy}') = 'array' then
                jsonb_set(with_csf.query, '{ordering,orderBy}', coalesce((
                    select
                        jsonb_agg(case jsonb_typeof(term.value)
                            when 'string' then
                                to_jsonb(query_name_rewrite.path_text_with_colon_names_quoted(term.value #>> '{}'))
                            else
                                term.value
                            end order by term.ordinal)
                    from jsonb_array_elements(with_csf.query #> '{ordering,orderBy}') with ordinality as term (value, ordinal)), '[]'::jsonb))
            else
                with_csf.query
            end
        from (
            select
                case when jsonb_typeof(with_q.query -> 'csf') = 'string' then
                    jsonb_set(with_q.query, '{csf}', to_jsonb(query_name_rewrite.path_text_with_colon_names_quoted(with_q.query ->> 'csf')))
                else
                    with_q.query
                end as query
            from (
                select
                    case when jsonb_typeof(query -> 'q') = 'string' then
                        jsonb_set(query, '{q}', to_jsonb(query_name_rewrite.path_text_with_colon_names_quoted(query ->> 'q')))
                    else
                        query
                    end as query) as with_q) as with_csf);

-- Whether one of the queries `queries` holds a name the rewrite quotes.
-- Stable, as the rewrite of one query is. Temporary, as only this file
-- calls it.
create function pg_temp.queries_hold_colon_names (queries json)
    returns boolean
    language sql
    stable
    parallel safe
    return exists (
        select
        from
            json_array_elements(queries) as written (query)
        where
            pg_temp.query_with_colon_names_quoted(written.query::jsonb) <> written.query::jsonb);

-- The queries `queries` in their order, each query holding a name the
-- rewrite quotes rewritten and every other query as written. Stable, as the
-- rewrite of one query is. Temporary, as only this file calls it.
create function pg_temp.queries_with_colon_names_quoted (queries json)
    returns json
    language sql
    stable
    parallel safe
    return (
        select
            json_agg(case when pg_temp.query_with_colon_names_quoted(written.query::jsonb) <> written.query::jsonb then
                    pg_temp.query_with_colon_names_quoted(written.query::jsonb)::json
                else
                    written.query
                end order by written.ordinal)
        from
            json_array_elements(queries) with ordinality as written (query, ordinal));

update
    snapshots
set
    snapshot_queries = pg_temp.queries_with_colon_names_quoted(snapshot_queries)
where
    pg_temp.queries_hold_colon_names(snapshot_queries);

update
    snapshots
set
    snapshot_temporal_queries = pg_temp.queries_with_colon_names_quoted(snapshot_temporal_queries)
where
    pg_temp.queries_hold_colon_names(snapshot_temporal_queries);

drop function pg_temp.queries_with_colon_names_quoted, pg_temp.queries_hold_colon_names, pg_temp.query_with_colon_names_quoted;
