-- ============================================================================
-- 0033: Seal-free attribute rows
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.2.2, 5.7.5-5.7.10
--
-- Converts the data-integrity seals of the branch it runs in, the live one or
-- a Snapshot's, with the tenant functions: in the current state, in history
-- and in the notification queue. A top-level `ngsildproof` instance leaves
-- the current state, its streams leave history, and the discovery and
-- temporal selection ledgers count them out as a write removing them would.
-- A history chunk in the columnstore is decompressed only for the streams
-- the rewrite names, so it names the streams holding a seal first, and lifts
-- the bound on the rows one statement may decompress.
--
-- No row is a sub-attribute, so no row keeps `proof_seals`. The check that a
-- tombstone holds nothing beside its deletion is rebuilt without it and
-- validated as it is added: TimescaleDB refuses to validate a constraint of
-- a hypertable whose columnstore is enabled. Every row already meets it, as
-- it is implied by the check it replaces.
-- ============================================================================
select
    set_config('timescaledb.max_tuples_decompressed_per_dml_transaction', '0', true);

do $$
declare
    seal_key integer := seal_attribute_name_key();
    streams bigint[];
begin
    if seal_key is null then
        return;
    end if;
    update
        current_attributes
    set
        sub_attributes = (typed_seal_tree(row (sub_attributes)::sub_attribute_tree, seal_key)).elements
    where
        exists (
            select
            from
                unnest(sub_attributes) element
            where
                element.attribute_name_key = seal_key);
    with removed as (
        delete from current_attributes
        where attribute_name_key = seal_key
        returning
            entity_key,
            attribute_kind
),
counted as (
    select
        entity_key,
        attribute_kind,
        count(*) as instance_count
    from
        removed
    group by
        entity_key,
        attribute_kind
),
searched as (
    update
        entity_search
    set
        attribute_name_keys = array_remove(attribute_name_keys, seal_key),
        attribute_instance_name_keys = array (
            select
                held.name_key
            from
                unnest(attribute_instance_name_keys, attribute_instance_kinds)
                with ordinality as held (name_key, kind, position)
            where
                held.name_key <> seal_key
            order by
                held.position),
            attribute_instance_kinds = array (
                select
                    held.kind
                from
                    unnest(attribute_instance_name_keys, attribute_instance_kinds)
                    with ordinality as held (name_key, kind, position)
                where
                    held.name_key <> seal_key
                order by
                    held.position),
                written_xid = pg_current_xact_id()
            where
                entity_key in (
                    select
                        entity_key
                    from
                        removed)
                returning
                    entity_key,
                    entity_type_name_keys
),
named as (
    insert into attribute_census (
        instance_delta,
        attribute_name_key)
    select
        - sum(instance_count),
        seal_key
    from
        counted
    having
        count(*) > 0)
insert into entity_type_attribute_census (
    instance_delta,
    entity_type_name_key,
    attribute_name_key,
    attribute_kind)
select
    - sum(counted.instance_count),
    typed.entity_type_name_key,
    seal_key,
    counted.attribute_kind
from
    counted
    join searched using (entity_key)
    cross join unnest(searched.entity_type_name_keys) as typed (entity_type_name_key)
group by
    typed.entity_type_name_key,
    counted.attribute_kind;
        streams := array ( select distinct
                stream_key
            from
                attribute_history
            where
                exists (
                    select
                    from
                        unnest(sub_attributes) element
                    where
                        element.attribute_name_key = seal_key));
        update
            attribute_history
        set
            sub_attributes = (typed_seal_tree(row (sub_attributes)::sub_attribute_tree, seal_key)).elements
        where
            stream_key = any (streams)
            and exists (
                select
                from
                    unnest(sub_attributes) element
                where
                    element.attribute_name_key = seal_key);
        streams := array (
            select
                stream_key
            from
                temporal_attribute_streams
            where
                attribute_name_key = seal_key);
        delete from attribute_history
        where stream_key = any (streams);
        delete from attribute_history_instance_claims
        where attribute_name_key = seal_key;
        with removed as (
            delete from temporal_attribute_streams
            where stream_key = any (streams)
            returning
                entity_key)
        insert into stream_type_census (
            stream_delta,
            entity_type_name_key,
            attribute_name_key)
        select
            - count(*),
            member.entity_type_name_key,
            seal_key
        from
            removed
            join entity_type_members member on member.entity_key = removed.entity_key
                and member.is_evolution_member
        group by
            member.entity_type_name_key;
        update
            notification_events
        set
            entity_snapshot = typed_seal_state(entity_snapshot, seal_key),
            previous_snapshot = typed_seal_state(previous_snapshot, seal_key);
end
$$;

alter table current_attributes
    drop column proof_seals;

alter table attribute_history
    drop column proof_seals cascade,
    add constraint chk_attribute_history_tombstone_members check (value_kind <> 0 or num_nonnulls(value_type_key, object_type_keys, unit_code_id, expires_at, sub_attributes) = 0);
