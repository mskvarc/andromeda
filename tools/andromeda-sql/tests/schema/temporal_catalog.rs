//! The shape of the temporal catalogue's attribute walk, stream extents and
//! shape masks.
//!
//! A temporal selection by attribute alone reads the holders of a sparse
//! attribute's streams from one index range, ordered by the Evolution's
//! surrogate key without a heap visit. A temporal geo query reads its
//! candidates through the extent index by the bare column and states the
//! index's predicate. Eligibility walks the distinct shape masks of an
//! attribute. The declared text of all three is pinned here.

use crate::repository::{PinError, constraint_definition, index, index_definition, schema, written};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};
use pretty_assertions::assert_eq;

/// The grain keys the default instance as the empty string, which no
/// `datasetId` may be, so it compares with plain equality, and it carries the
/// stream key and the `datasetId`, so resolving a grain and listing an
/// Entity's streams read the index alone.
#[test]
fn the_stream_grain_keys_the_default_instance_as_the_empty_string() -> Result<(), PinError> {
    let schema = schema()?;
    assert!(index(&schema, "uq_temporal_attribute_streams_grain")?.is_unique(), "the grain is unique");
    assert_eq!(
        index_definition(&schema, "uq_temporal_attribute_streams_grain")?,
        "on temporal_attribute_streams (entity_key, attribute_name_key, (coalesce(dataset_id, ''))) include (stream_key, dataset_id)"
    );
    assert_eq!(
        constraint_definition(&schema, "chk_temporal_attribute_streams_dataset_id_not_empty")?,
        "check (dataset_id <> '')"
    );
    Ok(())
}

#[test]
fn the_attribute_walk_index_orders_the_holders_by_their_evolution_key() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        index_definition(&schema, "idx_temporal_attribute_streams_attribute_name_key_entity_key")?,
        "on temporal_attribute_streams (attribute_name_key, entity_key, (coalesce(dataset_id, '')))"
    );
    Ok(())
}

/// A box test `"value_extent" && <box>` implies `value_extent is not null`,
/// which PostgreSQL proves for the strict `&&`, so the candidate read reaches
/// the partial index without stating the predicate itself.
#[test]
fn the_extent_test_names_the_indexed_expression() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        index_definition(&schema, "idx_temporal_attribute_streams_value_extent")?,
        "on temporal_attribute_streams using gist (value_extent) where value_extent is not null"
    );
    Ok(())
}

/// The mask owns one bit for every combination of the eight families of
/// clause 5.2.4 and the shapes Tables 4.5.19.1-1 to 4.5.19.1-3 tell apart,
/// and a stream that recorded no value holds none.
#[test]
fn the_shape_mask_holds_one_bit_per_family_and_shape() -> Result<(), PinError> {
    let schema = schema()?;
    let column = schema
        .columns()
        .iter()
        .find(|column| column.relation() == "temporal_attribute_streams" && column.name() == "shape_mask")
        .ok_or_else(|| PinError::Missing("the stream shape mask".to_owned()))?;
    let width = AttributeKind::ALL.len() * RecordedShape::ALL.len();
    assert_eq!(
        written(&schema, column.origin(), column.definition()),
        format!("bit({width}) compression lz4 not null default b'0'::bit({width})")
    );
    Ok(())
}

#[test]
fn the_shape_mask_index_ranges_over_the_masks_of_one_attribute() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(
        index_definition(&schema, "idx_temporal_attribute_streams_attribute_name_key_shape_mask")?,
        "on temporal_attribute_streams (attribute_name_key, shape_mask)"
    );
    Ok(())
}
