-- ============================================================================
-- 0028: Live dictionary references
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.6.1.4, 5.16.1.4
--
-- The references from the live relations to `attribute_names` and
-- `entity_type_names`: foreign keys, and for `current_attributes` a
-- statement-level check. They exist on the live
-- relations only, never on a Snapshot schema's copies.
--
-- A foreign key locks both of its ends. Declaring one takes `SHARE ROW
-- EXCLUSIVE` on the referenced table, and dropping one, which every
-- `DROP SCHEMA ... CASCADE` of a Snapshot schema does, takes `ACCESS
-- EXCLUSIVE` on it. On a Snapshot's copies these keys would make every
-- Snapshot creation, promotion, expiry and deletion lock the dictionaries
-- every live write reads, and PostgreSQL resolves the resulting cycles by
-- aborting one side: a live write failing because an unrelated Snapshot was
-- being prepared or dropped. Clause 5.6.1.4 fails a create only with the
-- errors it lists, and clause 5.16.1.4 prepares a Snapshot in the
-- background of the live context, so Snapshot DDL must never stall or abort a
-- live write.
--
-- A Snapshot's copies lose no integrity by the omission. The dictionaries
-- never delete or re-key a row, and a Snapshot is cloned from live rows whose
-- keys these references have already checked.
-- ============================================================================
alter table temporal_attribute_streams
    add constraint fk_temporal_attribute_streams_attribute_name foreign key (attribute_name_key) references attribute_names (attribute_name_key);

alter table attribute_history_instance_claims
    add constraint fk_attribute_history_instance_claims_attribute_name foreign key (attribute_name_key) references attribute_names (attribute_name_key);

alter table entity_type_members
    add constraint fk_entity_type_members_entity_type_name foreign key (entity_type_name_key) references entity_type_names (entity_type_name_key);

-- The attribute name references of `current_attributes`, those of its rows
-- and those of every element of `sub_attributes`, which no foreign key
-- reaches. Checked once per statement over its transition table,
-- in an insert and an update trigger, as the Entity references of the same
-- relation are.
create function check_current_attribute_name_keys ()
    returns trigger
    language plpgsql
    as $$
declare
    name_keys integer[];
    locked_count bigint;
begin
    name_keys := array(
        select written.attribute_name_key from new_rows written
        union
        select element.attribute_name_key from new_rows written cross join unnest(written.sub_attributes) element);
    select count(*) into locked_count
    from (select from public.attribute_names named where named.attribute_name_key = any (name_keys) for key share) locked;
    if locked_count < cardinality(name_keys) then
        raise foreign_key_violation
        using message = 'current attribute of no attribute name', table = tg_table_name;
    end if;
    return null;
end
$$;

create trigger trg_ca_name_keys_insert
    after insert on current_attributes referencing new table as new_rows for each statement
    execute function check_current_attribute_name_keys ();

create trigger trg_ca_name_keys_update
    after update on current_attributes referencing new table as new_rows for each statement
    execute function check_current_attribute_name_keys ();
