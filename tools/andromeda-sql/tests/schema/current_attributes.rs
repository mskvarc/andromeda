//! The shape of `current_attributes` and its statement-level references.
//!
//! The grain, the value placement, the partial value indexes, the sub-attribute
//! key column and its index and the triggers that stand in for foreign keys are
//! what every reader and writer of the relation relies on, so their declared
//! text is pinned here.

use crate::repository::{PinError, constraint_definition, function_definition, index, index_definition, schema, trigger, written};
use andromeda_sql::{
    declared_schema::{DeclaredSchema, ResolvedType, column::ColumnSource, constraint::ConstraintKind, declared_type::BuiltinType},
    migration_set::MigrationSet,
};
use pretty_assertions::assert_eq;

/// The relation the tests describe.
const TABLE: &str = "current_attributes";

/// Every typed value column, each the one `value_kind` pins.
const VALUE_COLUMNS: [&str; 9] = [
    "value_number",
    "value_string",
    "value_boolean",
    "value_geometry",
    "value_object",
    "value_date_time",
    "value_uri",
    "value_long_text",
    "value_long_uri",
];

/// The value kind codes and the columns each may populate.
const VALUE_KIND_COLUMNS: [(u8, &[&str]); 9] = [
    (1, &["value_number"]),
    (2, &["value_string", "value_long_text"]),
    (3, &["value_boolean"]),
    (4, &["value_geometry"]),
    (5, &["value_object"]),
    (6, &["value_date_time"]),
    (7, &["value_uri", "value_long_uri"]),
    (8, &["value_string"]),
    (9, &["value_string"]),
];

/// The fields of `relation`, each as `(name, type)`, but `excluded`.
fn fields<'schema>(schema: &'schema DeclaredSchema, relation: &str, excluded: &[&str]) -> Vec<(&'schema str, String)> {
    schema
        .columns()
        .iter()
        .filter(|column| column.relation() == relation && !excluded.contains(&column.name()))
        .map(|column| (column.name(), column.data_type().written()))
        .collect()
}

/// An instance holds as many sub-attribute instances as the value budgets
/// admit, so an element's parent position is an `integer`: a `smallint`
/// would refuse the 32,768th.
#[test]
fn an_element_names_its_parent_by_an_integer_position() -> Result<(), PinError> {
    let schema = schema()?;
    let first = schema
        .columns()
        .iter()
        .find(|column| column.relation() == "sub_attribute" && column.source() == ColumnSource::CompositeType)
        .ok_or_else(|| PinError::Missing("sub_attribute".to_owned()))?;
    assert_eq!(
        (first.name(), schema.resolve(first.data_type())),
        (
            "parent",
            ResolvedType::Builtin {
                element: BuiltinType::Int4,
                array: false
            }
        )
    );
    Ok(())
}

/// A queue holds an Entity's state as `attribute_instance_record`s, which
/// decode as `current_attributes` rows do: the record is the table's columns
/// but `entity_key` and the `sub_eq_keys`, `value_geometry_valid` and
/// `value_geometry_reach` a write derives from the instance, in order and with
/// the same types.
#[test]
fn an_instance_record_declares_the_current_attributes_columns_in_order() -> Result<(), PinError> {
    let schema = schema()?;
    let derived = ["entity_key", "sub_eq_keys", "value_geometry_valid", "value_geometry_reach"];
    let record = fields(&schema, "attribute_instance_record", &derived);
    assert!(!record.is_empty(), "no migration declares attribute_instance_record");
    assert_eq!(record, fields(&schema, TABLE, &derived));
    Ok(())
}

#[test]
fn every_value_kind_pins_exactly_one_column() -> Result<(), PinError> {
    let schema = schema()?;
    let presence = constraint_definition(&schema, "chk_current_attributes_value_presence")?;
    assert_eq!(presence, format!("check (num_nonnulls({}) = 1)", VALUE_COLUMNS.join(", ")));
    let placement = constraint_definition(&schema, "chk_current_attributes_value_kind_column")?;
    for (code, columns) in VALUE_KIND_COLUMNS {
        let arm = match columns {
            [column] => format!("(value_kind = {code} and {column} is not null)"),
            many => format!("(value_kind = {code} and num_nonnulls({}) = 1)", many.join(", ")),
        };
        assert!(placement.contains(&arm), "value kind {code} pins no column: {placement}");
    }
    assert!(!placement.contains("value_kind = 0"), "the tombstone never appears: {placement}");
    assert!(!placement.contains("value_kind = 10"), "there are nine value kinds: {placement}");
    Ok(())
}

/// The grain keys the default instance as the empty string, which no
/// `datasetId` may be, so it compares with plain equality.
#[test]
fn the_grain_is_entity_attribute_and_dataset() -> Result<(), PinError> {
    let schema = schema()?;
    let grain = index(&schema, "uq_current_attributes_grain")?;
    assert!(grain.is_unique(), "the grain is unique");
    assert_eq!(
        index_definition(&schema, "uq_current_attributes_grain")?,
        format!("on {TABLE} (entity_key, attribute_name_key, (coalesce(dataset_id, '')))")
    );
    assert_eq!(
        constraint_definition(&schema, "chk_current_attributes_dataset_id_not_empty")?,
        "check (dataset_id <> '')"
    );
    Ok(())
}

#[test]
fn every_value_index_is_partial_on_its_column() -> Result<(), PinError> {
    let schema = schema()?;
    let value_indexes: Vec<_> = schema.indexes().iter().filter(|index| index.name().starts_with("idx_ca_value_")).collect();
    assert_eq!(value_indexes.len(), 11, "{value_indexes:#?}");
    for index in value_indexes {
        let column = index
            .name()
            .strip_prefix("idx_ca_")
            .map(|column| column.strip_suffix("_trgm").unwrap_or(column))
            .unwrap_or_default();
        assert!(VALUE_COLUMNS.contains(&column), "{} keys no value column", index.name());
        let definition = written(&schema, index.origin(), index.definition());
        assert!(definition.starts_with(&format!("on {TABLE} ")), "{definition}");
        assert!(definition.ends_with(&format!("where {column} is not null")), "{definition}");
    }
    let geometry = index_definition(&schema, "idx_ca_entity_geometry")?;
    assert!(geometry.ends_with("where value_geometry is not null"), "{geometry}");
    Ok(())
}

#[test]
fn the_sub_attribute_key_index_is_partial_and_off_the_pending_list() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        index_definition(&schema, "idx_ca_sub_eq_keys")?,
        "on current_attributes using gin (sub_eq_keys) with (fastupdate = off) where sub_eq_keys is not null"
    );
    Ok(())
}

/// The most common pairs are what a probe naming one value kind of one
/// Attribute is priced from.
#[test]
fn the_attribute_value_kind_statistics_keep_the_most_common_pairs() -> Result<(), PinError> {
    let schema = schema()?;
    let statistics = schema
        .statistics()
        .iter()
        .find(|statistics| statistics.name() == "stx_ca_attribute_value_kind")
        .ok_or_else(|| PinError::Missing("stx_ca_attribute_value_kind".to_owned()))?;
    assert_eq!(
        written(&schema, statistics.origin(), statistics.definition()),
        "(dependencies, ndistinct, mcv) on attribute_name_key, value_kind from current_attributes"
    );
    Ok(())
}

/// A row without a keyed value stores NULL, which the partial index leaves
/// out, never an empty array it would index.
#[test]
fn the_sub_attribute_keys_are_null_or_non_empty() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        constraint_definition(&schema, "chk_current_attributes_sub_eq_keys_not_empty")?,
        "check (cardinality(sub_eq_keys) > 0)"
    );
    Ok(())
}

/// The keys are a plain column, so ANALYZE gathers its element statistics and
/// no object or function in the database computes them.
#[test]
fn no_statistics_object_or_function_computes_the_sub_attribute_keys() -> Result<(), PinError> {
    let schema = schema()?;
    assert!(!schema.statistics().iter().any(|statistics| statistics.name() == "stx_ca_sub_eq_keys"));
    for name in ["sub_eq_keys", "sub_eq_query_key", "sub_path_hash"] {
        assert!(!schema.functions().iter().any(|function| function.name().name() == name), "{name} is declared");
    }
    Ok(())
}

/// Concurrent writers queue on the posting pages of trigrams the strings of
/// one attribute share, so the string trigram index takes its inserts on a
/// pending list, bounded so a writer's flush and a read's scan stay short.
#[test]
fn the_string_trigram_index_is_on_a_bounded_pending_list() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        index_definition(&schema, "idx_ca_value_string_trgm")?,
        "on current_attributes using gin (attribute_name_key int4_ops, value_string gin_trgm_ops) with (fastupdate = on, gin_pending_list_limit = 64) where value_string is not null"
    );
    Ok(())
}

/// One long value yields about a thousand trigram keys, which would overflow
/// the bound alone.
#[test]
fn the_long_text_trigram_and_object_indexes_stay_off_the_pending_list() -> Result<(), PinError> {
    let schema = schema()?;
    for name in ["idx_ca_value_long_text_trgm", "idx_ca_value_object"] {
        let definition = index_definition(&schema, name)?;
        assert!(definition.contains(" with (fastupdate = off) "), "{name}: {definition}");
    }
    Ok(())
}

#[test]
fn instance_references_are_checked_per_statement() -> Result<(), PinError> {
    let schema = schema()?;
    assert!(
        !schema
            .constraints()
            .iter()
            .any(|constraint| constraint.table() == TABLE && constraint.kind() == ConstraintKind::ForeignKey),
        "current_attributes declares a foreign key"
    );
    for (what, checking) in [
        ("entities", "check_current_attribute_entities"),
        ("name_keys", "check_current_attribute_name_keys"),
    ] {
        for event in ["insert", "update"] {
            let declared = trigger(&schema, &format!("trg_{TABLE}_{what}_{event}"))?;
            assert_eq!(declared.table(), TABLE);
            assert_eq!(
                written(&schema, declared.origin(), declared.definition()),
                format!("after {event} on {TABLE} referencing new table as new_rows for each statement execute function {checking} ()")
            );
        }
    }
    let entities = function_definition(&schema, "check_current_attribute_entities")?;
    assert!(entities.contains("%I.entities where entity_key = any ($1) for key share"), "{entities}");
    assert!(entities.contains("raise foreign_key_violation"), "{entities}");
    let names = function_definition(&schema, "check_current_attribute_name_keys")?;
    for checked in [
        "public.attribute_names",
        "cross join unnest(written.sub_attributes) element",
        "for key share",
        "raise foreign_key_violation",
    ] {
        assert!(names.contains(checked), "the attribute name check lacks `{checked}`: {names}");
    }
    assert!(!names.contains("unit_code"), "a unit code references nothing in the database: {names}");
    Ok(())
}

#[test]
fn current_attributes_unit_code_id_is_checked_positive() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        constraint_definition(&schema, "chk_current_attributes_unit_code_id_positive")?,
        "check (unit_code_id > 0)"
    );
    Ok(())
}

#[test]
fn attribute_history_unit_code_id_is_checked_positive() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        constraint_definition(&schema, "chk_attribute_history_unit_code_id_positive")?,
        "check (unit_code_id > 0)"
    );
    Ok(())
}

#[test]
fn no_tenant_relation_is_named_unit_codes() -> Result<(), PinError> {
    let schema = schema()?;
    for table in ["unit_codes", "unit_code_catalog_metadata"] {
        assert!(!schema.tables().iter().any(|declared| declared.name() == table), "a migration declares {table}");
    }
    Ok(())
}

#[test]
fn the_entity_check_is_a_branch_relation_and_the_name_check_is_live_only() -> Result<(), PinError> {
    let schema = schema()?;
    for name in ["trg_current_attributes_entities_insert", "trg_current_attributes_entities_update"] {
        assert_eq!(schema.set_of(trigger(&schema, name)?.origin()), Some(MigrationSet::Branch), "{name}");
    }
    for name in ["trg_current_attributes_name_keys_insert", "trg_current_attributes_name_keys_update"] {
        assert_eq!(schema.set_of(trigger(&schema, name)?.origin()), Some(MigrationSet::Tenant), "{name}");
    }
    Ok(())
}

#[test]
fn the_entity_cascade_is_one_statement() -> Result<(), PinError> {
    let schema = schema()?;
    let cascade = trigger(&schema, "trg_entities_current_attributes_cascade")?;
    assert_eq!(cascade.table(), "entities");
    assert_eq!(
        written(&schema, cascade.origin(), cascade.definition()),
        "after delete on entities referencing old table as old_rows for each statement execute function cascade_entity_current_attributes ()"
    );
    let body = function_definition(&schema, "cascade_entity_current_attributes")?;
    assert!(body.contains("delete from %I.current_attributes where entity_key = any ($1)"), "{body}");
    assert!(body.contains("using array(select deleted.entity_key from old_rows deleted)"), "{body}");
    Ok(())
}
