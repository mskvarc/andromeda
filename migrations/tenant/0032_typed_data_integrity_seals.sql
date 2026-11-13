-- ============================================================================
-- 0032: Typed data-integrity seals
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.5.2.2
--
-- An `ngsildproof` is the seal of the attribute it is a sub-attribute of: a
-- Property whose non-reified subproperties `entityIdSealed` and
-- `entityTypeSealed` name the entity its W3C Data Integrity proof signs, and
-- whose value is that proof. ETSI TS 104 179 V1.1.1 clause 5.1.2 has a proof
-- hold at least its `type`, the fixed `DataIntegrityProof`, and
-- `cryptosuite`, `proofPurpose`, `verificationMethod`, `created` and
-- `proofValue`.
--
-- A seal is a `sub_attribute` element of value kind `data-integrity-proof`,
-- whose `value_object` is NULL. Its two subproperties are fields beside its
-- value: `seal_entity_id`, and `seal_entity_type_keys`, `entity_type_names`
-- keys in written order. The value is `value_proof`, a
-- `data_integrity_proof`.
--
-- Codes:
--
--   sub_attribute.value_kind: 12 data-integrity-proof
--
-- An `ngsildproof` held as an object value, with its subproperties in
-- `proof_seals`, is converted. One whose value is a single proof holding
-- every required member, and whose subproperties name an entity, becomes
-- typed. Any other one is dropped with its own sub-attributes: no write
-- admits it, so it is no seal. A top-level `ngsildproof` signs no attribute
-- and is dropped too. The queues of the tenant are converted here, the
-- branch set then converts every branch, the live one and each Snapshot's,
-- and only then does the tenant set remove `proof_seals` from the types. Only
-- a row that holds an `ngsildproof` is rewritten.
--
-- Every function named `seal_conversion_` serves this conversion and the
-- branch set's, and nothing else calls one: `seal_conversion_admits_uri`
-- admits fewer URIs than the broker, so a seal it refuses is dropped. The
-- two that convert the queues are temporary, so they end with the session
-- the migrations run in. The others stay, since every Snapshot branch
-- created later runs the branch set again, over relations that are still
-- empty.
-- ============================================================================
-- The W3C Data Integrity proof of a seal beside its `type`, which is always
-- `DataIntegrityProof`:
--
--   created              `created`, the instant
--   created_digits       the fractional-second digits `created` is written
--                        with, 0 to 6, which the signed literal keeps
--   cryptosuite          `cryptosuite`
--   proof_purpose        `proofPurpose`, its expanded IRI
--   verification_method  `verificationMethod`
--   proof_value          `proofValue`, as written
--   further_members      the proof's `@id` and every further member, in
--                        the structured value encoding of a node holding
--                        them, NULL when it has none
create type data_integrity_proof as (
    created timestamptz,
    created_digits smallint,
    cryptosuite text collate "C",
    proof_purpose text collate "C",
    verification_method text collate "C",
    proof_value text collate "C",
    further_members jsonb
);

alter type sub_attribute
    add attribute seal_entity_id text collate "C",
    add attribute seal_entity_type_keys integer[],
    add attribute value_proof data_integrity_proof;

-- The structured value of an element, its `value_kind` and the fields beside
-- it given one by one: the element is read from an unnest with ordinality,
-- whose row is no `sub_attribute`. It is `value_object`, or for a seal the
-- expanded proof node `value_proof` holds, as the model writes it. The year
-- is written from the ISO 8601 year, which counts a year 0 that the proleptic
-- calendar of `to_char` does not.
create function sub_attribute_value_object (value_kind smallint, value_object jsonb, value_proof data_integrity_proof)
    returns jsonb
    language sql
    stable
    parallel safe
    return case when value_kind = 12 then
        jsonb_build_object(
            '@type', jsonb_build_array('https://w3id.org/security#DataIntegrityProof'),
            'http://purl.org/dc/terms/created', jsonb_build_array(jsonb_build_object(
                '@type', 'http://www.w3.org/2001/XMLSchema#dateTime',
                '@value', lpad((extract(year from (value_proof).created at time zone 'UTC')::integer
                        + case when extract(year from (value_proof).created at time zone 'UTC') < 0 then 1 else 0 end)::text, 4, '0')
                    || to_char((value_proof).created at time zone 'UTC', '-MM-DD"T"HH24:MI:SS')
                    || case when (value_proof).created_digits > 0 then
                        '.' || left(to_char((value_proof).created at time zone 'UTC', 'US'), (value_proof).created_digits)
                    else
                        ''
                    end
                    || 'Z')),
            'https://w3id.org/security#verificationMethod', jsonb_build_array(jsonb_build_object(
                '@id', (value_proof).verification_method)),
            'https://w3id.org/security#cryptosuite', jsonb_build_array(jsonb_build_object(
                '@type', 'https://w3id.org/security#cryptosuiteString',
                '@value', (value_proof).cryptosuite)),
            'https://w3id.org/security#proofPurpose', jsonb_build_array(jsonb_build_object(
                '@id', (value_proof).proof_purpose)),
            'https://w3id.org/security#proofValue', jsonb_build_array(jsonb_build_object(
                '@type', 'https://w3id.org/security#multibase',
                '@value', (value_proof).proof_value)))
        || coalesce((value_proof).further_members, '{}')
    else
        value_object
    end;

-- The `attribute_names` key of `ngsildproof`, NULL while no instance has
-- named it.
create function seal_conversion_attribute_key ()
    returns integer
    language sql
    stable
    parallel safe
    return (
        select
            attribute_name_key
        from
            attribute_names
        where
            name = 'https://uri.etsi.org/ngsi-ld/ngsildproof');

-- Whether the broker accepts `uri`: an absolute IRI (IETF RFC 3987) that is
-- an `http` or `https` URL with a lowercase host name, an optional port and
-- no apostrophe, or one whose scheme is none of those and none of the other
-- special schemes of the WHATWG URL Standard (`file`, `ftp`, `ws`, `wss`),
-- with a non-empty path not starting with `/`. A port is any run of digits
-- whose value is at most 65535, leading zeros included, as the URL Standard
-- reads one. The broker reads an ASCII URI that is no URN with the WHATWG URL
-- parser and keeps the spelling the parser writes, which for `http` and
-- `https` drops a default port and the leading zeros of any other, resolves
-- dot segments, adds the `/` of an empty path and escapes an apostrophe in
-- the query. The added slash is the byte an identifier is kept below its
-- bound for. Each escaped apostrophe adds two bytes, which could take an
-- identifier past its bound, so no apostrophe is admitted. The parser
-- refuses some IRIs the grammar admits, so the test admits fewer than the
-- broker does and none it refuses.
create function seal_conversion_admits_uri (uri text)
    returns boolean
    language sql
    immutable
    parallel safe
    return uri ~ '^[a-z][a-z0-9+.-]*:'
    and uri !~ '[^A-Za-z0-9._~!$&''()*+,;=:@/?#%\u00a0-\ud7ff\uf900-\ufdcf\ufdf0-\uffef\U00010000-\U0001fffd\U00020000-\U0002fffd\U00030000-\U0003fffd\U00040000-\U0004fffd\U00050000-\U0005fffd\U00060000-\U0006fffd\U00070000-\U0007fffd\U00080000-\U0008fffd\U00090000-\U0009fffd\U000a0000-\U000afffd\U000b0000-\U000bfffd\U000c0000-\U000cfffd\U000d0000-\U000dfffd\U000e1000-\U000efffd-]'
    and uri !~ '%(?![0-9A-Fa-f]{2})'
    and uri !~ '#.*#'
    and case when split_part(uri, ':', 1) in ('http', 'https') then
        uri ~ '^https?://(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)*[a-z](?:[a-z0-9-]*[a-z0-9])?(?::0*(?:[0-9]{1,4}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?(?:[/?#]|$)'
        and uri !~ '^https?://(?:[^/?#]*\.)?xn--'
        and strpos(uri, '''') = 0
    when split_part(uri, ':', 1) in ('file', 'ftp', 'ws', 'wss') then
        false
    else
        uri ~ '^[a-z][a-z0-9+.-]*:[^/?#]'
    end;

-- The text of an expanded member's values when they are exactly one string
-- of `datatype`.
create function seal_conversion_typed_string (member_values jsonb, datatype text)
    returns text
    language sql
    immutable
    parallel safe
    return case when jsonb_typeof(member_values -> 0) = 'object'
        and member_values -> 1 is null then
        case when member_values -> 0 -> '@type' = to_jsonb(datatype)
            and jsonb_typeof(member_values -> 0 -> '@value') = 'string'
            and (member_values -> 0) - '@type' - '@value' = '{}'::jsonb then
            member_values -> 0 ->> '@value'
        end
    end;

-- The text of an expanded member's values when they are exactly one string
-- with no datatype, language or direction.
create function seal_conversion_plain_string (member_values jsonb)
    returns text
    language sql
    immutable
    parallel safe
    return case when jsonb_typeof(member_values -> 0) = 'object'
        and member_values -> 1 is null then
        case when jsonb_typeof(member_values -> 0 -> '@value') = 'string'
            and (member_values -> 0) - '@value' = '{}'::jsonb then
            member_values -> 0 ->> '@value'
        end
    end;

-- The IRI an expanded member's values name when they are exactly one node
-- reference.
create function seal_conversion_reference (member_values jsonb)
    returns text
    language sql
    immutable
    parallel safe
    return case when jsonb_typeof(member_values -> 0) = 'object'
        and member_values -> 1 is null then
        case when jsonb_typeof(member_values -> 0 -> '@id') = 'string'
            and (member_values -> 0) - '@id' = '{}'::jsonb then
            member_values -> 0 ->> '@id'
        end
    end;

-- The proof an expanded node is, NULL when it is none: its only type is
-- `DataIntegrityProof`, `created` is one clause 4.6.3 `DateTime`,
-- `cryptosuite` and `proofValue` are each one non-empty string of the
-- datatype the W3C Data Integrity v2 context coerces it to, and
-- `verificationMethod` and `proofPurpose` each reference one URI. ISO 8601
-- numbers the year before year 1 as 0, and PostgreSQL as -1, 1 BC, so that
-- year is made as -1.
create function seal_conversion_proof (node jsonb)
    returns data_integrity_proof
    language sql
    stable
    parallel safe
    return (
        select
            case when node -> '@type' = '["https://w3id.org/security#DataIntegrityProof"]'::jsonb
                and created[2]::integer between 1 and 12
                and created[3]::integer between 1 and 31
                and created[4]::integer <= 23
                and created[5]::integer <= 59
                and created[6]::integer <= 59
                and cryptosuite <> ''
                and proof_value <> ''
                and seal_conversion_admits_uri(proof_purpose)
                and seal_conversion_admits_uri(verification_method) then
                case when extract(month from make_date(calendar.year, created[2]::integer, 1) + (created[3]::integer - 1)) = created[2]::integer then
                    row (
                        make_timestamptz(calendar.year, created[2]::integer, created[3]::integer, created[4]::integer, created[5]::integer, 0, 'UTC')
                            + (created[6]::integer * 1000000 + coalesce(rpad(created[7], 6, '0')::integer, 0)) * interval '1 microsecond',
                        coalesce(char_length(created[7]), 0)::smallint,
                        cryptosuite,
                        proof_purpose,
                        verification_method,
                        proof_value,
                        nullif(node - array[
                            '@type',
                            'http://purl.org/dc/terms/created',
                            'https://w3id.org/security#cryptosuite',
                            'https://w3id.org/security#proofPurpose',
                            'https://w3id.org/security#proofValue',
                            'https://w3id.org/security#verificationMethod'], '{}'::jsonb))::data_integrity_proof
                end
            end
        from (
            select
                regexp_match(
                    seal_conversion_typed_string(node -> 'http://purl.org/dc/terms/created', 'http://www.w3.org/2001/XMLSchema#dateTime'),
                    '^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:[.,]([0-9]{1,6}))?Z$') as created,
                seal_conversion_typed_string(node -> 'https://w3id.org/security#cryptosuite', 'https://w3id.org/security#cryptosuiteString') as cryptosuite,
                seal_conversion_reference(node -> 'https://w3id.org/security#proofPurpose') as proof_purpose,
                seal_conversion_reference(node -> 'https://w3id.org/security#verificationMethod') as verification_method,
                seal_conversion_typed_string(node -> 'https://w3id.org/security#proofValue', 'https://w3id.org/security#multibase') as proof_value) proof_members
            cross join lateral (
                select
                    case when proof_members.created[1] = '0000' then -1 else proof_members.created[1]::integer end) as calendar (year));

-- The identifier `entityIdSealed` names in the subproperties of a seal, NULL
-- when it names none: one plain string that is an entity identifier other
-- than the NGSI-LD Null. The bound is one byte below the identifier bound,
-- the slash the broker adds to a URL with no path.
create function seal_conversion_entity_id (subproperties jsonb)
    returns text
    language sql
    immutable
    parallel safe
    return (
        select
            case when entity_id <> 'urn:ngsi-ld:null'
                and octet_length(entity_id) < 1024
                and seal_conversion_admits_uri(entity_id) then
                entity_id
            end
        from (
            select
                seal_conversion_plain_string(subproperties -> 'https://uri.etsi.org/ngsi-ld/entityIdSealed') as entity_id) sealed);

-- The type names `entityTypeSealed` names in the subproperties of a seal,
-- each once in written order, NULL unless it references at least one and
-- only URIs that are entity type names other than the NGSI-LD Null. A value
-- that is not a reference counts against the seal.
create function seal_conversion_entity_type_names (subproperties jsonb)
    returns text[]
    language sql
    immutable
    parallel safe
    return (
        select
            case when bool_and(coalesce(type_name <> 'urn:ngsi-ld:null'
                and octet_length(type_name) < 1024
                and seal_conversion_admits_uri(type_name), false)) then
                array_agg(type_name order by first_position)
            end
        from (
            select
                seal_conversion_reference(jsonb_build_array(sealed.type_value)) as type_name,
                min(sealed.position) as first_position
            from
                jsonb_array_elements(
                    case when jsonb_typeof(subproperties -> 'https://uri.etsi.org/ngsi-ld/entityTypeSealed') = 'array' then
                        subproperties -> 'https://uri.etsi.org/ngsi-ld/entityTypeSealed'
                    end)
                with ordinality as sealed (type_value, position)
            group by
                1) named);

-- The `entity_type_names` keys of `type_names`, in their order, each name
-- interned first if no key names it yet.
create function seal_conversion_intern_entity_types (type_names text[])
    returns integer[]
    language sql
    volatile
    begin atomic
        insert into entity_type_names (name)
        select
            type_name
        from
            unnest(type_names) as named (type_name)
        on conflict (name)
            do nothing;
        select
            array (
                select
                    entity_type_names.entity_type_name_key
                from
                    unnest(type_names)
                    with ordinality as named (type_name, position)
                    join entity_type_names on entity_type_names.name = named.type_name
                order by
                    named.position);
    end;

-- The sub-attributes of an instance with every `ngsildproof` converted:
-- typed when its value is a proof and its subproperties name an entity,
-- dropped with every element below it otherwise. A kept element's `parent`
-- is renumbered to its parent's new position, and no element left is no
-- array. The subproperties are read from the element's JSON image rather
-- than from a field, so the function outlives the field: once the types drop
-- `proof_seals`, a call reads none instead of failing. The elements travel in
-- a `sub_attribute_tree` because the PL/pgSQL parser the migrations are
-- linted with has no catalog: it reads a type it doesn't know as `record`
-- and an array of one as `record[]`, which no PL/pgSQL parameter, result or
-- variable may be.
create function seal_conversion_typed_tree (tree sub_attribute_tree, seal_key integer)
    returns sub_attribute_tree
    language plpgsql
    volatile
    as $$
declare
    element sub_attribute;
    kept sub_attribute_tree;
    positions integer[] := '{}';
    proof data_integrity_proof;
    subproperties jsonb;
    entity_id text;
    type_names text[];
begin
    if tree.elements is null then
        return tree;
    end if;
    kept.elements := '{}';
    foreach element in array tree.elements loop
        if element.parent > 0 and positions[element.parent] = 0 then
            positions := positions || 0;
            continue;
        end if;
        if element.attribute_name_key = seal_key then
            subproperties := to_jsonb(element) -> 'proof_seals';
            proof := case when element.attribute_kind = 1 and element.value_kind = 5 then seal_conversion_proof(element.value_object) end;
            entity_id := seal_conversion_entity_id(subproperties);
            type_names := seal_conversion_entity_type_names(subproperties);
            if proof is null or entity_id is null or type_names is null then
                positions := positions || 0;
                continue;
            end if;
            element.value_kind := 12;
            element.value_object := null;
            element.seal_entity_id := entity_id;
            element.seal_entity_type_keys := seal_conversion_intern_entity_types(type_names);
            element.value_proof := proof;
        end if;
        if element.parent > 0 then
            element.parent := positions[element.parent];
        end if;
        kept.elements := kept.elements || element;
        positions := positions || cardinality(kept.elements);
    end loop;
    if cardinality(kept.elements) = 0 then
        kept.elements := null;
    end if;
    return kept;
end
$$;

-- An Entity's state with every top-level `ngsildproof` instance dropped, the
-- ordinals behind it closed up, and the sub-attributes of the others
-- converted.
create function seal_conversion_typed_state (state entity_state_record, seal_key integer)
    returns entity_state_record
    language plpgsql
    volatile
    as $$
declare
    instance attribute_instance_record;
    converted entity_state_record := state;
    dropped integer := 0;
begin
    if state.instances is null then
        return state;
    end if;
    converted.instances := '{}';
    foreach instance in array state.instances loop
        if instance.attribute_name_key = seal_key then
            dropped := dropped + 1;
            continue;
        end if;
        instance.ordinal := instance.ordinal - dropped;
        instance.sub_attributes := (seal_conversion_typed_tree(row (instance.sub_attributes)::sub_attribute_tree, seal_key)).elements;
        converted.instances := converted.instances || instance;
    end loop;
    return converted;
end
$$;

-- Whether `state` holds an `ngsildproof`, as a top-level instance or among
-- the sub-attributes of one.
create function seal_conversion_state_holds_seal (state entity_state_record, seal_key integer)
    returns boolean
    language sql
    immutable
    parallel safe
    return exists (
        select
        from
            unnest((state).instances) as instance
        where
            instance.attribute_name_key = seal_key
            or exists (
                select
                from
                    unnest(instance.sub_attributes) as element
                where
                    element.attribute_name_key = seal_key));

-- The elements of `changes`, one of the three aligned arrays of a change, at
-- every position `name_keys`, the change's attribute name keys, names
-- something other than an `ngsildproof`: a change keeps no top-level seal
-- instance it touched, as its states keep none. PL/pgSQL, as a SQL-standard
-- body cannot take an array of any element type.
create function seal_conversion_kept_changes (changes anyarray, name_keys integer[], seal_key integer)
    returns anyarray
    language plpgsql
    immutable
    strict
    parallel safe
    as $$
begin
    return array (
        select
            changed.change
        from
            unnest(changes, name_keys)
            with ordinality as changed (change, name_key, position)
        where
            changed.name_key is distinct from seal_key
        order by
            changed.position);
end
$$;

-- A change a respond-async job owes with the seals of both of its states
-- converted and every top-level seal instance it touched left out of its
-- change arrays. The attribute name keys are filtered last, since the other
-- two arrays are filtered by them. Temporary, as only the queue conversion
-- here calls it.
create function pg_temp.seal_conversion_typed_event (event pending_change_event, seal_key integer)
    returns pending_change_event
    language plpgsql
    volatile
    as $$
begin
    event.entity_snapshot := seal_conversion_typed_state(event.entity_snapshot, seal_key);
    event.previous_snapshot := seal_conversion_typed_state(event.previous_snapshot, seal_key);
    event.change_kinds := seal_conversion_kept_changes(event.change_kinds, event.change_attribute_name_keys, seal_key);
    event.change_dataset_ids := seal_conversion_kept_changes(event.change_dataset_ids, event.change_attribute_name_keys, seal_key);
    event.change_attribute_name_keys := seal_conversion_kept_changes(event.change_attribute_name_keys, event.change_attribute_name_keys, seal_key);
    return event;
end
$$;

-- An entry of a temporal append intent with the seals of its sub-attributes
-- converted. Temporary, as only the queue conversion here calls it.
create function pg_temp.seal_conversion_typed_entry (entry temporal_entry, seal_key integer)
    returns temporal_entry
    language plpgsql
    volatile
    as $$
begin
    entry.sub_attributes := (seal_conversion_typed_tree(row (entry.sub_attributes)::sub_attribute_tree, seal_key)).elements;
    return entry;
end
$$;

update
    pending_side_effects
set
    events = array (
        select
            pg_temp.seal_conversion_typed_event(events[position], seal_key)
        from
            generate_subscripts(events, 1) as position
        order by
            position)
from
    seal_conversion_attribute_key() as seal_key
where
    seal_key is not null
    and exists (
        select
        from
            unnest(events) as queued
        where
            seal_key = any (queued.change_attribute_name_keys)
            or seal_conversion_state_holds_seal(queued.entity_snapshot, seal_key)
            or seal_conversion_state_holds_seal(queued.previous_snapshot, seal_key));

-- An attribute entry of a top-level `ngsildproof` is dropped. An entry's
-- write ordinal is its position, so the entries behind it take the ordinals
-- before them. `entry_needs` stays as the write recorded it. A need no entry
-- meets any longer runs its statement over no entry, while needs read anew
-- from the entries left would take an intent that only ended seal instances,
-- left with no entry, for an observation of the Entity its write never made.
update
    temporal_append_intents
set
    entries = array (
        select
            pg_temp.seal_conversion_typed_entry(entries[position], seal_key)
        from
            generate_subscripts(entries, 1) as position
        where (entries[position]).entry_kind not in (1, 2)
        or (entries[position]).attribute_name_key is distinct from seal_key
    order by
        position)
from
    seal_conversion_attribute_key() as seal_key
where
    seal_key is not null
    and exists (
        select
        from
            unnest(entries) as entry
        where
            entry.attribute_name_key = seal_key
            or exists (
                select
                from
                    unnest(entry.sub_attributes) as element
                where
                    element.attribute_name_key = seal_key));
