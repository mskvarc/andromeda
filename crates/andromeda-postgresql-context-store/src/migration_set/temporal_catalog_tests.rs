//! The shape of the temporal catalogue's attribute walk, checked against the
//! embedded migrations.
//!
//! A temporal selection by attribute alone pages the holders of the
//! attribute's streams in identifier order from one index range, and its
//! count re-orders them by the Evolution's surrogate key without a heap
//! visit, so the index's declared text is pinned here.

use crate::migration_set::declared_schema::DeclaredSchema;
use pretty_assertions::assert_eq;

/// `text` with every run of whitespace collapsed to one space.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_attribute_walk_index_orders_by_identifier_and_carries_the_evolution_key() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_temporal_attribute_streams_attribute_name_key_id")
        .unwrap_or_else(|| panic!("no migration declares the attribute walk index"));
    assert_eq!(
        normalized(&index.definition),
        "on temporal_attribute_streams (attribute_name_key, id) include (temporal_entity_key)"
    );
}
