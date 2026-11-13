-- ============================================================================
-- 0033: Typed data-integrity seals in branch rows
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.2.2, 5.7.5-5.7.10
--
-- Converts the data-integrity seals of the branch it runs in, the live one or
-- a Snapshot's, with the tenant's `seal_conversion_` functions: in the
-- current state, in history and in the notification queue. A top-level
-- `ngsildproof` instance leaves the current state, its streams leave
-- history, and the discovery and temporal selection ledgers count out the
-- rows each deletion returns, as a write removing them would: an instance
-- under the types of its Entity's search row, a stream under the types its
-- Entity's identity row records for the Evolution. The instances behind a
-- top-level `ngsildproof` close up their ordinals, and a queued event leaves
-- it out of the instances its change touched, as its states leave it out.
-- Only a row that holds an `ngsildproof` is rewritten, so a branch that never
-- named one changes nothing.
--
-- `sub_eq_keys` keeps the keys of the elements a conversion drops, those of a
-- tree left with none included. A key only selects candidates, and every
-- candidate is rechecked against its element. The keys are the broker's to
-- derive, so no statement here derives them again.
--
-- The conversion runs in one block, which reads the `ngsildproof` key once
-- and ends at once while no instance has named it. The tenant set drops the
-- `seal_conversion_` functions with `proof_seals`, and a Snapshot branch
-- created after that runs this file over relations that are still empty. A
-- statement of the block is resolved when it first runs, so each statement
-- that calls one of the functions runs only where its relation holds a row:
-- an empty branch never names a dropped function, and a branch that still
-- holds rows fails rather than lose its seals unconverted.
--
-- A history chunk in the columnstore is decompressed only for the streams a
-- statement names as a bound value. A plain statement can name them only
-- through a subquery, which decompresses every batch, so the block reads the
-- streams into variables first, and the bound on the rows one statement may
-- decompress is lifted. The top-level instances leave the current state, and
-- their streams leave history, before the rest is rewritten, so no row is
-- rewritten only to be deleted.
--
-- No row is a sub-attribute, so no row keeps `proof_seals`. The check that a
-- tombstone holds nothing beside its deletion names the column, so dropping
-- the column drops the check with it. The check is added again without the
-- column and `not valid`, so no row is read to add it: every row already
-- meets it, as it is implied by the check it replaces, and every row written
-- later is tested against it. It is never validated, which would read every
-- row to learn nothing, and which TimescaleDB refuses on a hypertable whose
-- columnstore is enabled.
-- ============================================================================
select
    set_config('timescaledb.max_tuples_decompressed_per_dml_transaction', '0', true);

do $$
declare
    seal_key integer := (
        select
            attribute_name_key
        from
            attribute_names
        where
            name = 'https://uri.etsi.org/ngsi-ld/ngsildproof');
    top_level_streams bigint[];
    sealed_streams bigint[];
begin
    if seal_key is null then
        return;
    end if;

    with removed as (
        delete from current_attributes
        where attribute_name_key = seal_key
        returning
            entity_key,
            attribute_name_key,
            attribute_kind,
            ordinal
    ),
    closed as (
        update
            current_attributes
        set
            ordinal = current_attributes.ordinal - behind.dropped
        from (
            select
                later.entity_key,
                later.attribute_name_key,
                later.dataset_key,
                count(*) as dropped
            from
                removed
                join current_attributes as later on later.entity_key = removed.entity_key
                    and later.attribute_name_key <> removed.attribute_name_key
                    and later.ordinal > removed.ordinal
            group by
                later.entity_key,
                later.attribute_name_key,
                later.dataset_key) as behind
        where
            current_attributes.entity_key = behind.entity_key
            and current_attributes.attribute_name_key = behind.attribute_name_key
            and current_attributes.dataset_key = behind.dataset_key
    ),
    counted as (
        select
            entity_key,
            attribute_name_key,
            attribute_kind,
            count(*) as instance_count
        from
            removed
        group by
            entity_key,
            attribute_name_key,
            attribute_kind
    ),
    searched as (
        update
            entity_search
        set
            attribute_name_keys = array_remove(attribute_name_keys, sealed.attribute_name_key),
            attribute_instance_name_keys = coalesce((
                select
                    array_agg(held.name_key order by held.position)
                from
                    unnest(attribute_instance_name_keys, attribute_instance_kinds)
                    with ordinality as held (name_key, kind, position)
                where
                    held.name_key <> sealed.attribute_name_key), '{}'),
            attribute_instance_kinds = coalesce((
                select
                    array_agg(held.kind order by held.position)
                from
                    unnest(attribute_instance_name_keys, attribute_instance_kinds)
                    with ordinality as held (name_key, kind, position)
                where
                    held.name_key <> sealed.attribute_name_key), '{}'),
            written_xid = pg_current_xact_id()
        from (
            select distinct
                entity_key,
                attribute_name_key
            from
                removed) as sealed
        where
            entity_search.entity_key = sealed.entity_key
        returning
            entity_search.entity_key,
            entity_search.entity_type_name_keys
    ),
    named as (
        insert into attribute_census (
            instance_delta,
            attribute_name_key)
        select
            - sum(instance_count),
            attribute_name_key
        from
            counted
        group by
            attribute_name_key
    )
    insert into entity_type_attribute_census (
        instance_delta,
        entity_type_name_key,
        attribute_name_key,
        attribute_kind)
    select
        - sum(counted.instance_count),
        typed.entity_type_name_key,
        counted.attribute_name_key,
        counted.attribute_kind
    from
        counted
        join searched using (entity_key)
        cross join unnest(searched.entity_type_name_keys) as typed (entity_type_name_key)
    group by
        typed.entity_type_name_key,
        counted.attribute_name_key,
        counted.attribute_kind;

    if exists (
        select
        from
            current_attributes) then
        update
            current_attributes
        set
            sub_attributes = (seal_conversion_typed_tree(row (sub_attributes)::sub_attribute_tree, seal_key)).elements
        where
            exists (
                select
                from
                    unnest(sub_attributes) element
                where
                    element.attribute_name_key = seal_key);
    end if;

    top_level_streams := array (
        select
            stream_key
        from
            temporal_attribute_streams
        where
            attribute_name_key = seal_key);

    delete from attribute_history
    where stream_key = any (top_level_streams);

    sealed_streams := array (
        select distinct
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

    if cardinality(sealed_streams) > 0 then
        update
            attribute_history
        set
            sub_attributes = (seal_conversion_typed_tree(row (sub_attributes)::sub_attribute_tree, seal_key)).elements
        where
            stream_key = any (sealed_streams)
            and exists (
                select
                from
                    unnest(sub_attributes) element
                where
                    element.attribute_name_key = seal_key);
    end if;

    delete from attribute_history_instance_claims
    where attribute_name_key = seal_key;

    with removed as (
        delete from temporal_attribute_streams
        where attribute_name_key = seal_key
        returning
            entity_key,
            attribute_name_key
    )
    insert into stream_type_census (
        stream_delta,
        entity_type_name_key,
        attribute_name_key)
    select
        - count(*),
        typed.entity_type_name_key,
        removed.attribute_name_key
    from
        removed
        join entities on entities.entity_key = removed.entity_key
        cross join unnest(entities.evolution_type_name_keys) as typed (entity_type_name_key)
    group by
        typed.entity_type_name_key,
        removed.attribute_name_key;

    if exists (
        select
        from
            notification_events) then
        update
            notification_events
        set
            change_kinds = seal_conversion_kept_changes(change_kinds, change_attribute_name_keys, seal_key),
            change_attribute_name_keys = seal_conversion_kept_changes(change_attribute_name_keys, change_attribute_name_keys, seal_key),
            change_dataset_ids = seal_conversion_kept_changes(change_dataset_ids, change_attribute_name_keys, seal_key),
            entity_snapshot = seal_conversion_typed_state(entity_snapshot, seal_key),
            previous_snapshot = seal_conversion_typed_state(previous_snapshot, seal_key)
        where
            seal_key = any (change_attribute_name_keys)
            or seal_conversion_state_holds_seal(entity_snapshot, seal_key)
            or seal_conversion_state_holds_seal(previous_snapshot, seal_key);
    end if;
end
$$;

alter table current_attributes
    drop column proof_seals;

alter table attribute_history
    drop column proof_seals,
    add constraint chk_attribute_history_tombstone_members check (value_kind <> 0 or num_nonnulls(value_type_key, object_type_keys, unit_code_id, expires_at, sub_attributes) = 0) not valid;
