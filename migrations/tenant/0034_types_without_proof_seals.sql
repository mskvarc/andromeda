-- ============================================================================
-- 0034: Types without proof_seals
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.5.2.2
--
-- No top-level instance, temporal entry or row is a data-integrity seal, and
-- a seal element holds its non-reified subproperties in fields of their own,
-- so no type keeps `proof_seals`. Every branch is converted, so nothing
-- calls a `seal_conversion_` function again, and they are dropped in one
-- statement, as some of them call others. The view of the queued temporal
-- entries names every field of an entry, so it is dropped before the types
-- change and created again after them.
-- ============================================================================
drop function seal_conversion_typed_state, seal_conversion_typed_tree, seal_conversion_state_holds_seal, seal_conversion_kept_changes, seal_conversion_intern_entity_types, seal_conversion_entity_type_names, seal_conversion_entity_id, seal_conversion_proof, seal_conversion_reference, seal_conversion_plain_string, seal_conversion_typed_string, seal_conversion_admits_uri, seal_conversion_attribute_key;

drop view temporal_append_entries;

alter type temporal_entry
    drop attribute proof_seals;

alter type attribute_instance_record
    drop attribute proof_seals;

alter type sub_attribute
    drop attribute proof_seals;

-- Every entry of every intent beside the write identity its history row will
-- carry: `write_ordinal` is the intent's `base_ordinal` plus the entry's
-- position counted from 0, as the application numbers it. A temporal read of
-- the live branch unions these entries with the recorded history, so an entry
-- answers the same whether or not it has been applied yet.
create view temporal_append_entries as
select
    i.intent_key,
    i.entity_id,
    i.recorded_at,
    i.write_id,
    (i.base_ordinal + e.ordinality - 1)::integer as write_ordinal,
    e.entry_kind,
    e.attribute_name_key,
    e.dataset_key,
    e.attribute_kind,
    e.value_kind,
    e.observed_at,
    e.created_at,
    e.modified_at,
    e.deleted_at,
    e.value_date_time,
    e.expires_at,
    e.generated_instance_id,
    e.value_type_key,
    e.value_datatype_key,
    e.unit_code_id,
    e.value_boolean,
    e.verbatim_instance_id,
    e.value_number,
    e.value_string,
    e.value_language,
    e.value_geometry,
    e.value_geometry_reach,
    e.value_geometry_members,
    e.value_object,
    e.value_uri,
    e.object_type_keys,
    e.sub_attributes,
    e.scope_path,
    e.verbatim_scope_path
from
    temporal_append_intents i
    cross join lateral unnest(i.entries)
    with ordinality as e;
