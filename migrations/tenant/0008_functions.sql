-- ============================================================================
-- 0008: Functions every branch resolves
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.9
--
-- The functions the branch relations name in a generated column or an index
-- expression. They live in `public`, and a Snapshot schema reaches them
-- through its search path, so one definition serves every branch.
--
-- The body of a function an index expression calls names every object outside
-- `pg_catalog` with its schema. `VACUUM`, `ANALYZE`, `REINDEX` and `CREATE
-- INDEX` evaluate an index expression with `search_path = pg_catalog,
-- pg_temp`, so an unqualified name inside it does not resolve there.
-- ============================================================================
-- `geom_to_geog` wraps the `geometry` to `geography` cast so a stored
-- generated column can use it. PostGIS marks the cast `STABLE` because it
-- reads `spatial_ref_sys`, but for SRID 4326, the only SRID the schema
-- admits, the result is deterministic, so the wrapper is declared `IMMUTABLE`.
create or replace function geom_to_geog (g geometry)
    returns geography
    language sql
    immutable strict parallel safe
    as $$
    select
        g::geography
$$;

-- The hash of a sub-attribute's name path: the `attribute_names` keys from the
-- top-level attribute down to the sub-attribute, each folded into the hash of
-- the path above it.
create or replace function sub_path_hash (path integer[])
    returns bigint
    language plpgsql
    immutable strict parallel safe
    as $$
declare
    path_hash bigint := 0;
    name_key integer;
begin
    foreach name_key in array path loop
        path_hash := hashint8extended(name_key, path_hash);
    end loop;
    return path_hash;
end
$$;

-- The equality keys of an instance's sub-attributes, which a GIN expression
-- index holds so a clause 4.9 `q` equality on a sub-attribute reads the index.
-- One key per element holding one scalar value: the hash of the element's
-- name path, salted with its value kind code, combined with the hash of its
-- value. A number hashes through its `numeric` hash, so `1.0` and `1.00`
-- share a key; a DateTime as its epoch microseconds. An element whose
-- `value_object` is an array (several values, several objects, a language
-- map, a list) has one key per scalar member, a value node read through its
-- `@value`, because a `q` equality matches a value the array includes; JSON
-- has no instant, so a DateTime member keys as its text. A string element
-- holding a clause 4.6.3 time of day also keys as the canonical fixed-width
-- text `HH:MM:SS.ffffffZ` a `q` comparison normalises it to; the shape check
-- admits only text carrying its own offset, so neither the cast nor `to_char`
-- depends on a session setting. An element whose value is a geometry or a
-- structured object has no key. A key selects candidates only: two values may
-- share a key, so a query rechecks the value it compares.
create or replace function sub_eq_keys (attribute_name_key integer, sub_attributes public.sub_attribute[])
    returns bigint[]
    language plpgsql
    immutable parallel safe
    as $$
declare
    root_hash bigint := hashint8extended(attribute_name_key, 0);
    path_hashes bigint[] := '{}';
    keys bigint[] := '{}';
    element public.sub_attribute;
    element_position integer := 0;
    path_hash bigint;
    member jsonb;
    member_value jsonb;
    time_of_day text;
begin
    if sub_attributes is null then
        return null;
    end if;
    foreach element in array sub_attributes loop
        element_position := element_position + 1;
        if element.parent = 0 then
            path_hash := hashint8extended(element.attribute_name_key, root_hash);
        else
            path_hash := hashint8extended(element.attribute_name_key, path_hashes[element.parent]);
        end if;
        path_hashes[element_position] := path_hash;
        if element.value_number is not null then
            keys := keys || hash_numeric_extended(element.value_number, path_hash # 1);
        elsif element.value_string is not null then
            keys := keys || hashtextextended(element.value_string, path_hash # 2);
            if element.value_string ~ '^[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$' and pg_input_is_valid('1970-01-01T' || element.value_string, 'timestamptz') then
                time_of_day := to_char(('1970-01-01T' || element.value_string)::timestamptz at time zone 'UTC', 'HH24:MI:SS.US') || 'Z';
                if time_of_day <> element.value_string then
                    keys := keys || hashtextextended(time_of_day, path_hash # 2);
                end if;
            end if;
        elsif element.value_boolean is not null then
            keys := keys || hashint8extended(element.value_boolean::integer, path_hash # 3);
        elsif element.value_date_time is not null then
            keys := keys || hashint8extended((extract(epoch from element.value_date_time) * 1000000)::bigint, path_hash # 6);
        elsif element.value_uri is not null then
            keys := keys || hashtextextended(element.value_uri, path_hash # 7);
        elsif jsonb_typeof(element.value_object) = 'array' then
            for member in
            select
                value
            from
                jsonb_array_elements(element.value_object)
                loop
                    member_value := case when jsonb_typeof(member) = 'object' then
                        member -> '@value'
                    else
                        member
                    end;
                    case jsonb_typeof(member_value)
                    when 'number' then
                        keys := keys || hash_numeric_extended((member_value #>> '{}')::numeric, path_hash # 1);
                    when 'string' then
                        keys := keys || hashtextextended(member_value #>> '{}', path_hash # 2);
                    when 'boolean' then
                        keys := keys || hashint8extended((member_value #>> '{}')::boolean::integer, path_hash # 3);
                    else
                        null;
                    end case;
                end loop;
        end if;
    end loop;
    return keys;
end
$$;

-- The one equality key a `q` comparison of a number at a sub-attribute name
-- path has, the top-level attribute's key first in `path`.
create or replace function sub_eq_query_key (path integer[], value numeric)
    returns bigint[]
    language sql
    immutable strict parallel safe
    as $$
    select
        array[hash_numeric_extended(value, public.sub_path_hash(path) # 1)]
$$;

create or replace function sub_eq_query_key (path integer[], value boolean)
    returns bigint[]
    language sql
    immutable strict parallel safe
    as $$
    select
        array[hashint8extended(value::integer, public.sub_path_hash(path) # 3)]
$$;

create or replace function sub_eq_query_key (path integer[], value timestamptz)
    returns bigint[]
    language sql
    immutable strict parallel safe
    as $$
    select
        array[hashint8extended((extract(epoch from value) * 1000000)::bigint, public.sub_path_hash(path) # 6)]
$$;

-- A String (`value_kind` 2) or a URI (`value_kind` 7): the kind salts the key,
-- so a String never shares a key with a URI of the same text.
create or replace function sub_eq_query_key (path integer[], value_kind smallint, value text)
    returns bigint[]
    language sql
    immutable strict parallel safe
    as $$
    select
        array[hashtextextended(value, public.sub_path_hash(path) # value_kind)]
$$;

-- Refuses every change to a dictionary but the ones its triggers let through.
-- Keys of the dictionaries are held where no foreign key reaches, so they are
-- correct only while a key, once minted, names the same value for the life of
-- the tenant. Serves the row triggers and the statement triggers alike.
create or replace function refuse_dictionary_change ()
    returns trigger
    language plpgsql
    as $$
begin
    raise restrict_violation
    using message = tg_table_name || ' is append-only: a key once minted names the same value for the life of the tenant', table = tg_table_name;
end
$$;

