//! The stored Context Source Property document of a registration, checked
//! against the embedded migrations.
//!
//! Registration discovery evaluates a clause 4.9 `csf` against this one
//! column, and every registration write fills it, so a row without it is a row
//! no `csf` could ever read.

use crate::migration_set::declared_schema::DeclaredSchema;

#[test]
fn every_registration_stores_its_context_source_property_document() {
    let schema = DeclaredSchema::of_every_migration();
    let column = schema
        .columns
        .iter()
        .find(|column| column.table == "context_source_registrations" && column.name == "context_source_properties")
        .unwrap_or_else(|| panic!("no migration declares the Context Source Property document"));
    pretty_assertions::assert_eq!(
        column.definition.split_whitespace().collect::<Vec<_>>().join(" "),
        "jsonb compression lz4 not null"
    );
}
