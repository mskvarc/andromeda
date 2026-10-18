//! The shape of the temporal catalogue's attribute walk and stream extents,
//! checked against the embedded migrations.
//!
//! A temporal selection by attribute alone pages the holders of the
//! attribute's streams in identifier order from one index range, and its
//! count re-orders them by the Evolution's surrogate key without a heap
//! visit. A temporal geo query reads its candidates through the extent index
//! by the bare column and states the index's predicate. Both indexes'
//! declared text is pinned here.

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

/// A box test `"value_extent" && <box>` implies `value_extent is not null`,
/// which PostgreSQL proves for the strict `&&`, so the candidate read reaches
/// the partial index without stating the predicate itself.
#[test]
fn the_extent_test_names_the_indexed_expression() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_temporal_attribute_streams_value_extent")
        .unwrap_or_else(|| panic!("no migration declares the stream extent index"));
    assert_eq!(
        normalized(&index.definition),
        "on temporal_attribute_streams using gist (value_extent) where value_extent is not null"
    );
}
