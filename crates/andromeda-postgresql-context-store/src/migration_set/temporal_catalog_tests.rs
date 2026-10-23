//! The shape of the temporal catalogue's attribute walk, stream extents and
//! shape masks, checked against the embedded migrations.
//!
//! A temporal selection by attribute alone reads the holders of a sparse
//! attribute's streams from one index range, ordered by the Evolution's
//! surrogate key without a heap visit. A temporal geo query reads its candidates through the extent index
//! by the bare column and states the index's predicate. Eligibility walks the
//! distinct shape masks of an attribute. The declared text of all three is
//! pinned here.

use crate::migration_set::declared_schema::DeclaredSchema;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};
use pretty_assertions::assert_eq;

/// `text` with every run of whitespace collapsed to one space.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_attribute_walk_index_orders_the_holders_by_their_evolution_key() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_temporal_attribute_streams_attribute_name_key_entity_key")
        .unwrap_or_else(|| panic!("no migration declares the attribute walk index"));
    assert_eq!(normalized(&index.definition), "on temporal_attribute_streams (attribute_name_key, entity_key)");
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

/// The mask owns one bit for every combination of the eight families of
/// clause 5.2.4 and the shapes Tables 4.5.19.1-1 to 4.5.19.1-3 tell apart,
/// and a stream that recorded no value holds none.
#[test]
fn the_shape_mask_holds_one_bit_per_family_and_shape() {
    let schema = DeclaredSchema::of_every_migration();
    let column = schema
        .columns
        .iter()
        .find(|column| column.table == "temporal_attribute_streams" && column.name == "shape_mask")
        .unwrap_or_else(|| panic!("no migration declares the stream shape mask"));
    let width = AttributeKind::ALL.len() * RecordedShape::ALL.len();
    assert_eq!(normalized(&column.definition), format!("bit({width}) not null default B'0'::bit({width})"));
}

#[test]
fn the_shape_mask_index_ranges_over_the_masks_of_one_attribute() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_temporal_attribute_streams_attribute_name_key_shape_mask")
        .unwrap_or_else(|| panic!("no migration declares the shape mask index"));
    assert_eq!(normalized(&index.definition), "on temporal_attribute_streams (attribute_name_key, shape_mask)");
}
