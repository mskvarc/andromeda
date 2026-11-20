//! The functions and schemas the migrations leave behind. A function or schema
//! a migration creates for one conversion is dropped by the migration that
//! ends it, so the broker's schema holds only what its statements and triggers
//! call.

use crate::repository::{PinError, schema};
use andromeda_sql::declared_schema::{
    created_object::{CreatedObject, ObjectKind},
    function::DeclaredFunction,
    qualified_name::QualifiedName,
};
use pretty_assertions::assert_eq;

/// Every function the migrations create and leave in place, schema-qualified
/// where the migration qualifies it, in the order created.
#[test]
fn only_the_dictionary_triggers_the_attribute_checks_and_the_structured_value_reader_survive() -> Result<(), PinError> {
    let schema = schema()?;
    let dropped = |kind: ObjectKind, name: &QualifiedName| schema.dropped().iter().any(|dropped| dropped.kind() == kind && dropped.name() == name);
    let surviving: Vec<String> = schema
        .functions()
        .iter()
        .map(DeclaredFunction::name)
        .filter(|name| !dropped(ObjectKind::Function, name))
        .filter(|name| {
            name.schema()
                .is_none_or(|qualifier| !dropped(ObjectKind::Schema, &QualifiedName::unqualified(qualifier)))
        })
        .map(|name| {
            name.schema()
                .map_or_else(|| name.name().to_owned(), |qualifier| format!("{qualifier}.{}", name.name()))
        })
        .collect();
    assert_eq!(
        surviving,
        [
            "refuse_dictionary_change",
            "check_current_attribute_entities",
            "cascade_entity_current_attributes",
            "check_current_attribute_name_keys",
            "sub_attribute_value_object",
        ]
    );
    Ok(())
}

/// No schema a migration creates outlives the migrations.
#[test]
fn every_schema_a_migration_creates_is_dropped_again() -> Result<(), PinError> {
    let schema = schema()?;
    let surviving: Vec<&str> = schema
        .created()
        .iter()
        .filter(|object| object.kind() == ObjectKind::Schema)
        .map(CreatedObject::name)
        .filter(|name| {
            !schema
                .dropped()
                .iter()
                .any(|dropped| dropped.kind() == ObjectKind::Schema && dropped.name().name() == *name)
        })
        .collect();
    assert_eq!(surviving, Vec::<&str>::new());
    Ok(())
}
