-- ============================================================================
-- 0032: Typed data-integrity seals
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.5.2.2
--
-- An `ngsildproof` is the seal of the attribute it is a sub-attribute of: a
-- Property whose `entityIdSealed` and `entityTypeSealed` name the entity its
-- W3C Data Integrity proof signs, and whose value is that proof. ETSI TS 104
-- 179 V1.1.1 clause 5.1.2 has a proof hold at least its `type`, the fixed
-- `DataIntegrityProof`, and `cryptosuite`, `proofPurpose`,
-- `verificationMethod`, `created` and `proofValue`.
--
-- A seal is a `sub_attribute` element of `value_kind` 12,
-- `data-integrity-proof`, whose `value_object` is NULL. The two seals are
-- members of the Property beside its value: `seal_entity_id`, and
-- `seal_entity_type_keys`, `entity_type_names` keys in written order. The
-- value is `value_proof`, a `data_integrity_proof`.
--
-- An `ngsildproof` held as an object value beside a `proof_seals` member is
-- converted. One whose value is a single proof holding every required member,
-- and whose seals name an entity, becomes typed. Any other one is dropped with
-- its own sub-attributes: no write admits it, so it is no seal. A top-level
-- `ngsildproof` signs no attribute and is dropped too. This migration
-- converts the queues of the tenant, the branch set then converts every
-- branch, the live one and each Snapshot's, and only then does the tenant set
-- remove `proof_seals` from the types. The functions the conversion calls
-- stay, since every Snapshot branch created later runs the branch set again.
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

-- The `attribute_names` key of `ngsildproof`, NULL while no instance has
-- named it.
create function seal_attribute_name_key ()
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

-- Whether the broker reads `uri` back as the URI it is: an absolute IRI (IETF
-- RFC 3987) that is an `http` or `https` URL with a lowercase host name, an
-- optional port and no apostrophe, or names any other scheme with a path not
-- starting with `/`. The WHATWG URL parser the broker reads an ASCII URI
-- with refuses some IRIs the grammar admits, so the test admits fewer than
-- the broker does and none it refuses.
create function admits_uri (uri text)
    returns boolean
    language sql
    immutable
    parallel safe
    return uri ~ '^[a-z][a-z0-9+.-]*:'
    and uri !~ '[^A-Za-z0-9._~!$&''()*+,;=:@/?#% -퟿豈-﷏ﷰ-￯\U00010000-\U0001fffd\U00020000-\U0002fffd\U00030000-\U0003fffd\U00040000-\U0004fffd\U00050000-\U0005fffd\U00060000-\U0006fffd\U00070000-\U0007fffd\U00080000-\U0008fffd\U00090000-\U0009fffd\U000a0000-\U000afffd\U000b0000-\U000bfffd\U000c0000-\U000cfffd\U000d0000-\U000dfffd\U000e1000-\U000efffd-]'
    and uri !~ '%(?![0-9A-Fa-f]{2})'
    and uri !~ '#.*#'
    and case when split_part(uri, ':', 1) in ('http', 'https') then
        uri ~ '^https?://(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)*[a-z](?:[a-z0-9-]*[a-z0-9])?(?::[0-9]{1,4})?(?:[/?#]|$)'
        and uri !~ '^https?://(?:[^/?#]*\.)?xn--'
        and strpos(uri, '''') = 0
    when split_part(uri, ':', 1) in ('file', 'ftp', 'ws', 'wss') then
        false
    else
        uri ~ '^[a-z][a-z0-9+.-]*:[^/?#]'
    end;

-- The text of an expanded member's values when they are exactly one string
-- of `datatype`.
create function typed_string_of (member_values jsonb, datatype text)
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
create function plain_string_of (member_values jsonb)
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
create function reference_of (member_values jsonb)
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
-- `DataIntegrityProof`, `created` is one clause 4.6.3 `DateTime` of year 1
-- or later, `cryptosuite` and `proofValue` are each one non-empty string of
-- the datatype the W3C Data Integrity v2 context coerces it to, and
-- `verificationMethod` and `proofPurpose` each reference one URI.
create function data_integrity_proof_of (node jsonb)
    returns data_integrity_proof
    language sql
    stable
    parallel safe
    return (
        select
            case when node -> '@type' = '["https://w3id.org/security#DataIntegrityProof"]'::jsonb
                and created[1]::integer >= 1
                and created[2]::integer between 1 and 12
                and created[3]::integer between 1 and 31
                and created[4]::integer <= 23
                and created[5]::integer <= 59
                and created[6]::integer <= 59
                and cryptosuite <> ''
                and proof_value <> ''
                and admits_uri(proof_purpose)
                and admits_uri(verification_method) then
                case when extract(month from make_date(created[1]::integer, created[2]::integer, 1) + (created[3]::integer - 1)) = created[2]::integer then
                    row (make_timestamptz(created[1]::integer, created[2]::integer, created[3]::integer, created[4]::integer, created[5]::integer, 0, 'UTC') + (created[6]::integer * 1000000 + coalesce(rpad(created[7], 6, '0')::integer, 0)) * interval '1 microsecond', coalesce(char_length(created[7]), 0)::smallint, cryptosuite, proof_purpose, verification_method, proof_value, nullif(node - array['@type', 'http://purl.org/dc/terms/created', 'https://w3id.org/security#cryptosuite', 'https://w3id.org/security#proofPurpose', 'https://w3id.org/security#proofValue', 'https://w3id.org/security#verificationMethod'], '{}'::jsonb))::data_integrity_proof
                end
            end
        from (
            select
                regexp_match(typed_string_of(node -> 'http://purl.org/dc/terms/created', 'http://www.w3.org/2001/XMLSchema#dateTime'), '^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:[.,]([0-9]{1,6}))?Z$') as created,
                typed_string_of(node -> 'https://w3id.org/security#cryptosuite', 'https://w3id.org/security#cryptosuiteString') as cryptosuite,
                reference_of(node -> 'https://w3id.org/security#proofPurpose') as proof_purpose,
                reference_of(node -> 'https://w3id.org/security#verificationMethod') as verification_method,
                typed_string_of(node -> 'https://w3id.org/security#proofValue', 'https://w3id.org/security#multibase') as proof_value) proof_members);

-- The identifier `entityIdSealed` names in the seals of an instance, NULL
-- when it names none: one plain string that is an entity identifier other
-- than the NGSI-LD Null. The bound is one byte below the identifier bound,
-- the slash the broker adds to a URL with no path.
create function sealed_entity_id (seals jsonb)
    returns text
    language sql
    immutable
    parallel safe
    return (
        select
            case when entity_id <> 'urn:ngsi-ld:null'
                and octet_length(entity_id) < 1024
                and admits_uri(entity_id) then
                entity_id
            end
        from (
            select
                plain_string_of(seals -> 'https://uri.etsi.org/ngsi-ld/entityIdSealed') as entity_id) sealed);

-- The type names `entityTypeSealed` names in the seals of an instance, each
-- once in written order, NULL unless it references at least one and only
-- URIs that are entity type names other than the NGSI-LD Null. A value that
-- is not a reference counts against the seal.
create function sealed_entity_type_names (seals jsonb)
    returns text[]
    language sql
    immutable
    parallel safe
    return (
        select
            case when bool_and(coalesce(type_name <> 'urn:ngsi-ld:null'
                and octet_length(type_name) < 1024
                and admits_uri(type_name), false)) then
                array_agg(type_name order by first_position)
            end
        from (
            select
                reference_of(jsonb_build_array(sealed.type_value)) as type_name,
                min(sealed.position) as first_position
            from
                jsonb_array_elements(
                    case when jsonb_typeof(seals -> 'https://uri.etsi.org/ngsi-ld/entityTypeSealed') = 'array' then
                        seals -> 'https://uri.etsi.org/ngsi-ld/entityTypeSealed'
                    end)
                with ordinality as sealed (type_value, position)
            group by
                1) named);

-- The `entity_type_names` keys of `type_names`, in their order, each name
-- interned first if no key names it yet.
create function entity_type_name_keys_of (type_names text[])
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
-- typed when its value is a proof and its seals name an entity, dropped with
-- every element below it otherwise. A kept element's `parent` is renumbered
-- to its parent's new position, and no element left is no array. The seals
-- are read from the element's JSON image rather than from a field, so the
-- function outlives the field: once the types drop `proof_seals`, a call
-- finds no seal instead of failing.
create function typed_seal_tree (tree sub_attribute_tree, seal_key integer)
    returns sub_attribute_tree
    language plpgsql
    volatile
    as $$
declare
    element sub_attribute;
    kept sub_attribute_tree;
    positions integer[] := '{}';
    proof data_integrity_proof;
    seals jsonb;
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
            seals := to_jsonb(element) -> 'proof_seals';
            proof := case when element.attribute_kind = 1 and element.value_kind = 5 then data_integrity_proof_of(element.value_object) end;
            entity_id := sealed_entity_id(seals);
            type_names := sealed_entity_type_names(seals);
            if proof is null or entity_id is null or type_names is null then
                positions := positions || 0;
                continue;
            end if;
            element.value_kind := 12;
            element.value_object := null;
            element.seal_entity_id := entity_id;
            element.seal_entity_type_keys := entity_type_name_keys_of(type_names);
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
create function typed_seal_state (state entity_state_record, seal_key integer)
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
        instance.sub_attributes := (typed_seal_tree(row (instance.sub_attributes)::sub_attribute_tree, seal_key)).elements;
        converted.instances := converted.instances || instance;
    end loop;
    return converted;
end
$$;

-- A change a respond-async job owes with the seals of both of its states
-- converted.
create function typed_seal_event (event pending_change_event, seal_key integer)
    returns pending_change_event
    language plpgsql
    volatile
    as $$
begin
    event.entity_snapshot := typed_seal_state(event.entity_snapshot, seal_key);
    event.previous_snapshot := typed_seal_state(event.previous_snapshot, seal_key);
    return event;
end
$$;

-- An entry of a temporal append intent with the seals of its sub-attributes
-- converted.
create function typed_seal_entry (entry temporal_entry, seal_key integer)
    returns temporal_entry
    language plpgsql
    volatile
    as $$
begin
    entry.sub_attributes := (typed_seal_tree(row (entry.sub_attributes)::sub_attribute_tree, seal_key)).elements;
    return entry;
end
$$;

update
    pending_side_effects
set
    events = array (
        select
            typed_seal_event(events[position], seal_attribute_name_key())
        from
            generate_subscripts(events, 1) as position
        order by
            position)
    where
        seal_attribute_name_key() is not null;

-- An attribute entry of a top-level `ngsildproof` is dropped. An entry's
-- write ordinal is its position, so the entries behind it take the ordinals
-- before them.
update
    temporal_append_intents
set
    entries = array (
        select
            typed_seal_entry(entries[position], seal_attribute_name_key())
        from
            generate_subscripts(entries, 1) as position
        where (entries[position]).entry_kind not in (1, 2)
        or (entries[position]).attribute_name_key is distinct from seal_attribute_name_key()
    order by
        position)
where
    seal_attribute_name_key() is not null;

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
        jsonb_build_object('@type', jsonb_build_array('https://w3id.org/security#DataIntegrityProof'), 'http://purl.org/dc/terms/created', jsonb_build_array(jsonb_build_object('@type', 'http://www.w3.org/2001/XMLSchema#dateTime', '@value', lpad((extract(year from (value_proof).created at time zone 'UTC')::integer + case when extract(year from (value_proof).created at time zone 'UTC') < 0 then 1 else 0 end)::text, 4, '0') || to_char((value_proof).created at time zone 'UTC', '-MM-DD"T"HH24:MI:SS') || case when (value_proof).created_digits > 0 then '.' || left(to_char((value_proof).created at time zone 'UTC', 'US'), (value_proof).created_digits) else '' end || 'Z')), 'https://w3id.org/security#verificationMethod', jsonb_build_array(jsonb_build_object('@id', (value_proof).verification_method)), 'https://w3id.org/security#cryptosuite', jsonb_build_array(jsonb_build_object('@type', 'https://w3id.org/security#cryptosuiteString', '@value', (value_proof).cryptosuite)), 'https://w3id.org/security#proofPurpose', jsonb_build_array(jsonb_build_object('@id', (value_proof).proof_purpose)), 'https://w3id.org/security#proofValue', jsonb_build_array(jsonb_build_object('@type', 'https://w3id.org/security#multibase', '@value', (value_proof).proof_value))) || coalesce((value_proof).further_members, '{}')
    else
        value_object
    end;
