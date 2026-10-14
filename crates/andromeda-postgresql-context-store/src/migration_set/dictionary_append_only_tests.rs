//! The append-only rule of the dictionaries, checked against the embedded
//! migrations.
//!
//! Dictionary keys are held where no foreign key reaches (sub-attribute
//! elements, queue records, every Snapshot schema's copies), so a key is
//! correct only while it names the same value for the life of the tenant. The
//! triggers that refuse every other change are what makes that true, so their
//! declared text is pinned here.

use crate::migration_set::declared_schema::{DeclaredSchema, DeclaredTrigger};
use pretty_assertions::assert_eq;

/// The trigger function every dictionary guard executes.
const REFUSAL: &str = "refuse_dictionary_change";

/// `text` with every run of whitespace collapsed to one space.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The trigger `name`, which a migration must declare.
fn trigger<'a>(schema: &'a DeclaredSchema, name: &str) -> &'a DeclaredTrigger {
    schema
        .triggers
        .iter()
        .find(|trigger| trigger.name == name)
        .unwrap_or_else(|| panic!("no migration declares {name}"))
}

/// The normalized definition of the trigger `name`, which must fire on `table`.
fn definition_on(schema: &DeclaredSchema, name: &str, table: &str) -> String {
    let declared = trigger(schema, name);
    assert_eq!(declared.table, table, "{name} fires on the wrong table");
    normalized(&declared.definition)
}

#[test]
fn both_name_dictionaries_refuse_update_delete_and_truncate() {
    let schema = DeclaredSchema::of_every_migration();
    for table in ["attribute_names", "entity_type_names"] {
        assert_eq!(
            definition_on(&schema, &format!("trg_{table}_append_only_row"), table),
            format!("before update or delete on {table} for each row execute function {REFUSAL} ()")
        );
        assert_eq!(
            definition_on(&schema, &format!("trg_{table}_append_only_truncate"), table),
            format!("before truncate on {table} for each statement execute function {REFUSAL} ()")
        );
    }
    let [refusal] = schema.functions.iter().filter(|function| function.name == REFUSAL).collect::<Vec<_>>()[..] else {
        panic!("{REFUSAL} is declared exactly once");
    };
    let body = normalized(&refusal.definition);
    assert!(body.starts_with("() returns trigger language plpgsql"), "{body}");
    assert!(body.contains("raise restrict_violation"), "the refusal raises SQLSTATE 23001: {body}");
    assert!(body.contains("is append-only"), "{body}");
}
