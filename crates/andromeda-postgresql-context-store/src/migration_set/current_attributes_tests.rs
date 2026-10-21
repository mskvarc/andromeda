//! The shape of `current_attributes` and its statement-level references,
//! checked against the embedded migrations.
//!
//! The grain, the value placement, the partial value indexes, the sub-attribute
//! key column and its index and the triggers that stand in for foreign keys are
//! what every reader and writer of the relation relies on, so their declared
//! text is pinned here.

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

/// The one declaration of the function `name`, which a migration must declare.
fn function<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a DeclaredFunction {
    let [function] = schema.functions.iter().filter(|function| function.name == name).collect::<Vec<_>>()[..] else {
        panic!("{name} is declared exactly once");
    };
    function
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

/// A queue holds an Entity's state as `attribute_instance_record`s, which
/// decode as `current_attributes` rows do: the record is the table's columns
/// but `entity_key` and the `sub_eq_keys` a write derives from the instance,
/// in order and with the same types.
#[test]
fn an_instance_record_declares_the_current_attributes_columns_in_order() {
    let schema = DeclaredSchema::of_every_migration();
    let fields = |table: &str, columns: &[DeclaredColumn]| {
        columns
            .iter()
            .filter(|column| column.table == table && !["entity_key", "sub_eq_keys"].contains(&column.name.as_str()))
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
        "on current_attributes using gin (sub_eq_keys) with (fastupdate = off) where sub_eq_keys is not null"
    );
}

/// A row without a keyed value stores NULL, which the partial index leaves
/// out, never an empty array it would index.
#[test]
fn the_sub_attribute_keys_are_null_or_non_empty() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(
        normalized(constraint(&schema, "chk_current_attributes_sub_eq_keys_not_empty")),
        "check (cardinality(sub_eq_keys) > 0)"
    );
}

/// The keys are a plain column, so ANALYZE gathers its element statistics and
/// no object or function in the database computes them.
#[test]
fn no_statistics_object_or_function_computes_the_sub_attribute_keys() {
    let schema = DeclaredSchema::of_every_migration();
    assert!(!schema.indexes.iter().any(|index| index.name == "stx_ca_sub_eq_keys"));
    for name in ["sub_eq_keys", "sub_eq_query_key", "sub_path_hash"] {
        assert!(!schema.functions.iter().any(|function| function.name == name), "{name} is declared");
    }
}

/// Concurrent writers queue on the posting pages of trigrams the strings of
/// one attribute share, so the string trigram index takes its inserts on a
/// pending list, bounded so a writer's flush and a read's scan stay short.
#[test]
fn the_string_trigram_index_is_on_a_bounded_pending_list() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(
        normalized(&index(&schema, "idx_ca_value_string_trgm").definition),
        "on current_attributes using gin (attribute_name_key int4_ops, value_string gin_trgm_ops) with (fastupdate = on, gin_pending_list_limit = 64) where value_string is not null"
    );
}

/// One long value yields about a thousand trigram keys, which would overflow
/// the bound alone.
#[test]
fn the_long_text_trigram_and_object_indexes_stay_off_the_pending_list() {
    let schema = DeclaredSchema::of_every_migration();
    for name in ["idx_ca_value_long_text_trgm", "idx_ca_value_object"] {
        let definition = normalized(&index(&schema, name).definition);
        assert!(definition.contains(" with (fastupdate = off) "), "{name}: {definition}");
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
        ("name_keys", "check_current_attribute_name_keys"),
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
    let names = normalized(&function(&schema, "check_current_attribute_name_keys").definition);
    for checked in [
        "public.attribute_names",
        "cross join unnest(written.sub_attributes) element",
        "for key share",
        "raise foreign_key_violation",
    ] {
        assert!(names.contains(checked), "the attribute name check lacks `{checked}`: {names}");
    }
    assert!(!names.contains("unit_code"), "a unit code references nothing in the database: {names}");
}

#[test]
fn current_attributes_unit_code_id_is_checked_positive() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(
        normalized(constraint(&schema, "chk_current_attributes_unit_code_id_positive")),
        "check (unit_code_id > 0)"
    );
}

#[test]
fn attribute_history_unit_code_id_is_checked_positive() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(
        normalized(constraint(&schema, "chk_attribute_history_unit_code_id_positive")),
        "check (unit_code_id > 0)"
    );
}

#[test]
fn no_tenant_relation_is_named_unit_codes() {
    let schema = DeclaredSchema::of_every_migration();
    for table in ["unit_codes", "unit_code_catalog_metadata"] {
        assert!(!schema.tables.iter().any(|declared| declared == table), "a migration declares {table}");
    }
}

#[test]
fn the_entity_check_is_a_branch_relation_and_the_name_check_is_live_only() {
    let branch: String = branch_migrations().iter().map(|migration| migration.sql.as_str()).collect();
    assert!(branch.contains("create trigger trg_current_attributes_entities_insert"));
    assert!(branch.contains("create trigger trg_current_attributes_entities_update"));
    assert!(!branch.contains("trg_current_attributes_name_keys"));
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
