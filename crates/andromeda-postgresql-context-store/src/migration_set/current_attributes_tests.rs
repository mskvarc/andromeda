//! The shape of `current_attributes`, its key functions and its
//! statement-level references, checked against the embedded migrations.
//!
//! The grain, the value placement, the partial value indexes, the sub-attribute
//! key index with its statistics and the triggers that stand in for foreign
//! keys are what every reader and writer of the relation relies on, so their
//! declared text is pinned here.

use crate::migration_set::{
    branch::branch_migrations,
    declared_schema::{ConstraintKind, DeclaredColumn, DeclaredFunction, DeclaredIndex, DeclaredSchema, DeclaredTrigger, every_migration_sql},
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
const VALUE_KIND_COLUMNS: [(u8, &[&str]); 7] = [
    (1, &["value_number"]),
    (2, &["value_string", "value_long_text"]),
    (3, &["value_boolean"]),
    (4, &["value_geometry"]),
    (5, &["value_object"]),
    (6, &["value_date_time"]),
    (7, &["value_uri", "value_long_uri"]),
];

/// `text` with every run of whitespace collapsed to one space.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The constraint `name`, which a migration must declare.
fn constraint<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a str {
    schema
        .constraints
        .iter()
        .find(|constraint| constraint.name == name)
        .unwrap_or_else(|| panic!("no migration declares {name}"))
        .definition
        .as_str()
}

/// The index or statistics object `name`, which a migration must declare.
fn index<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a DeclaredIndex {
    schema
        .indexes
        .iter()
        .find(|index| index.name == name)
        .unwrap_or_else(|| panic!("no migration declares {name}"))
}

/// The trigger `name`, which a migration must declare.
fn trigger<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a DeclaredTrigger {
    schema
        .triggers
        .iter()
        .find(|trigger| trigger.name == name)
        .unwrap_or_else(|| panic!("no migration declares {name}"))
}

/// Every declaration of the function `name`, one per overload.
fn functions<'a>(schema: &'a DeclaredSchema, name: &str) -> Vec<&'a DeclaredFunction> {
    schema.functions.iter().filter(|function| function.name == name).collect()
}

/// The one declaration of the function `name`, which a migration must declare.
fn function<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a DeclaredFunction {
    let [function] = functions(schema, name)[..] else {
        panic!("{name} is declared exactly once");
    };
    function
}

/// The expression `idx_ca_sub_eq_keys` keys, parenthesised as written.
fn sub_attribute_key_expression(schema: &DeclaredSchema) -> String {
    let definition = normalized(&index(schema, "idx_ca_sub_eq_keys").definition);
    definition
        .split_once("using gin (")
        .and_then(|(_, rest)| rest.split_once(") with"))
        .unwrap_or_else(|| panic!("the key index is a GIN over one expression: {definition}"))
        .0
        .to_owned()
}

/// An instance holds as many sub-attribute instances as the value budgets
/// admit, so an element's parent position is an `integer`: a `smallint`
/// would refuse the 32,768th.
#[test]
fn an_element_names_its_parent_by_an_integer_position() {
    let fields = every_migration_sql()
        .into_iter()
        .map(|sql| normalized(sql).to_lowercase())
        .find_map(|sql| sql.split_once("create type sub_attribute as (").map(|(_, fields)| fields.to_owned()))
        .unwrap_or_else(|| panic!("no migration declares sub_attribute"));
    assert!(fields.trim_start().starts_with("parent integer,"), "{fields}");
}

/// A queue holds an Entity's state as `attribute_instance_record`s, which SQL
/// moves into `current_attributes` rows field for field: the record is the
/// table's columns but `entity_key`, in order and with the same types.
#[test]
fn an_instance_record_declares_the_current_attributes_columns_in_order() {
    let schema = DeclaredSchema::of_every_migration();
    let fields = |table: &str, columns: &[DeclaredColumn]| {
        columns
            .iter()
            .filter(|column| column.table == table && column.name != "entity_key")
            .map(|column| (column.name.clone(), column.type_word()))
            .collect::<Vec<_>>()
    };
    let record = fields("attribute_instance_record", &schema.composite_fields);
    assert!(!record.is_empty(), "no migration declares attribute_instance_record");
    assert_eq!(record, fields(TABLE, &schema.columns));
}

#[test]
fn every_value_kind_pins_exactly_one_column() {
    let schema = DeclaredSchema::of_every_migration();
    let presence = normalized(constraint(&schema, "chk_current_attributes_value_presence"));
    assert_eq!(presence, format!("check (num_nonnulls ({}) = 1)", VALUE_COLUMNS.join(", ")));
    let placement = normalized(constraint(&schema, "chk_current_attributes_value_kind_column"));
    for (code, columns) in VALUE_KIND_COLUMNS {
        let arm = match columns {
            [column] => format!("(value_kind = {code} and {column} is not null)"),
            many => format!("(value_kind = {code} and num_nonnulls ({}) = 1)", many.join(", ")),
        };
        assert!(placement.contains(&arm), "value kind {code} pins no column: {placement}");
    }
    assert!(!placement.contains("value_kind = 0"), "the tombstone never appears: {placement}");
    assert!(!placement.contains("value_kind = 8"), "there are seven value kinds: {placement}");
}

#[test]
fn the_grain_is_entity_attribute_and_dataset() {
    let schema = DeclaredSchema::of_every_migration();
    let grain = schema
        .constraints
        .iter()
        .find(|constraint| constraint.name == "uq_current_attributes_grain")
        .unwrap_or_else(|| panic!("no migration declares the grain"));
    assert_eq!((grain.table.as_str(), grain.kind), (TABLE, ConstraintKind::Unique));
    assert_eq!(
        normalized(&grain.definition),
        "unique nulls not distinct (entity_key, attribute_name_key, dataset_id)"
    );
}

#[test]
fn every_value_index_is_partial_on_its_column() {
    let schema = DeclaredSchema::of_every_migration();
    let value_indexes: Vec<&DeclaredIndex> = schema.indexes.iter().filter(|index| index.name.starts_with("idx_ca_value_")).collect();
    assert_eq!(value_indexes.len(), 11, "{value_indexes:#?}");
    for index in value_indexes {
        let column = index
            .name
            .strip_prefix("idx_ca_")
            .map(|column| column.strip_suffix("_trgm").unwrap_or(column))
            .unwrap_or_default();
        assert!(VALUE_COLUMNS.contains(&column), "{} keys no value column", index.name);
        let definition = normalized(&index.definition);
        assert!(definition.starts_with(&format!("on {TABLE} ")), "{definition}");
        assert!(definition.ends_with(&format!("where {column} is not null")), "{definition}");
    }
    let geometry = normalized(&index(&schema, "idx_ca_entity_geometry").definition);
    assert!(geometry.ends_with("where value_geometry is not null"), "{geometry}");
}

#[test]
fn the_sub_attribute_key_index_is_partial_and_off_the_pending_list() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(
        normalized(&index(&schema, "idx_ca_sub_eq_keys").definition),
        "on current_attributes using gin (sub_eq_keys(attribute_name_key, sub_attributes)) with (fastupdate = off) where sub_attributes is not null"
    );
}

#[test]
fn the_sub_attribute_key_expression_has_statistics() {
    let schema = DeclaredSchema::of_every_migration();
    let statistics = index(&schema, "stx_ca_sub_eq_keys");
    assert_eq!(statistics.table, TABLE);
    assert_eq!(
        normalized(&statistics.definition),
        format!("on ({}) from {TABLE}", sub_attribute_key_expression(&schema))
    );
}

#[test]
fn the_key_functions_are_immutable_and_parallel_safe() {
    let schema = DeclaredSchema::of_every_migration();
    let query_keys = functions(&schema, "sub_eq_query_key");
    assert_eq!(query_keys.len(), 4, "one query key per value kind family: {query_keys:#?}");
    let declarations = query_keys
        .into_iter()
        .chain([function(&schema, "sub_eq_keys"), function(&schema, "sub_path_hash")]);
    for declaration in declarations {
        let attributes = normalized(&declaration.definition);
        let attributes = attributes.split_once(" as $$").map_or(attributes.as_str(), |(attributes, _)| attributes);
        assert!(attributes.contains(" immutable "), "{} is not immutable: {attributes}", declaration.name);
        assert!(
            attributes.ends_with(" parallel safe"),
            "{} is not parallel safe: {attributes}",
            declaration.name
        );
    }
}

#[test]
fn instance_references_are_checked_per_statement() {
    let schema = DeclaredSchema::of_every_migration();
    assert!(
        !schema
            .constraints
            .iter()
            .any(|constraint| constraint.table == TABLE && constraint.kind == ConstraintKind::ForeignKey),
        "current_attributes declares a foreign key"
    );
    let pairs = [
        ("entities", "check_current_attribute_entities"),
        ("dictionary_keys", "check_current_attribute_dictionary_keys"),
    ];
    for (what, checking) in pairs {
        for event in ["insert", "update"] {
            let declared = trigger(&schema, &format!("trg_{TABLE}_{what}_{event}"));
            assert_eq!(declared.table, TABLE);
            assert_eq!(
                normalized(&declared.definition),
                format!("after {event} on {TABLE} referencing new table as new_rows for each statement execute function {checking} ()")
            );
        }
    }
    let entities = normalized(&function(&schema, "check_current_attribute_entities").definition);
    assert!(entities.contains("%I.entities where entity_key = any ($1) for key share"), "{entities}");
    assert!(entities.contains("raise foreign_key_violation"), "{entities}");
    let dictionaries = normalized(&function(&schema, "check_current_attribute_dictionary_keys").definition);
    for checked in [
        "public.attribute_names",
        "public.unit_codes",
        "cross join unnest(written.sub_attributes) element",
        "for key share",
        "raise foreign_key_violation",
    ] {
        assert!(dictionaries.contains(checked), "the dictionary check lacks `{checked}`: {dictionaries}");
    }
}

#[test]
fn the_entity_check_is_a_branch_relation_and_the_dictionary_check_is_live_only() {
    let branch: String = branch_migrations().iter().map(|migration| migration.sql.as_str()).collect();
    assert!(branch.contains("create trigger trg_current_attributes_entities_insert"));
    assert!(branch.contains("create trigger trg_current_attributes_entities_update"));
    assert!(!branch.contains("trg_current_attributes_dictionary_keys"));
}

#[test]
fn the_entity_cascade_is_one_statement() {
    let schema = DeclaredSchema::of_every_migration();
    let cascade = trigger(&schema, "trg_entities_current_attributes_cascade");
    assert_eq!(cascade.table, "entities");
    assert_eq!(
        normalized(&cascade.definition),
        "after delete on entities referencing old table as old_rows for each statement execute function cascade_entity_current_attributes ()"
    );
    let body = normalized(&function(&schema, "cascade_entity_current_attributes").definition);
    assert!(body.contains("delete from %I.current_attributes where entity_key = any ($1)"), "{body}");
    assert!(body.contains("using array(select deleted.entity_key from old_rows deleted)"), "{body}");
}
