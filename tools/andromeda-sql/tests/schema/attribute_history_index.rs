//! The indexes of `attribute_history`.
//!
//! Clause 5.7.4.4 checks a geoquery against the `GeoProperty` instances of each
//! Entity the temporal query selects, so every spatial read of history is one
//! Evolution's rows, reached through the stream indexes. A geometry index over
//! the whole table is only ever intersected with that lookup, which is why its
//! absence is pinned here.

use crate::repository::{PinError, index, index_definition, schema};
use andromeda_sql::declared_schema::index::DeclaredIndex;

/// Clause 4.11: the in-effect read bounds the placement from above, so a row
/// with neither `observedAt` nor `deletedAt` is never read on that axis.
#[test]
fn the_observed_at_placement_index_skips_rows_placed_nowhere() -> Result<(), PinError> {
    let schema = schema()?;
    pretty_assertions::assert_eq!(index(&schema, "idx_attribute_history_stream_observed_placement")?.table(), "attribute_history");
    let definition = index_definition(&schema, "idx_attribute_history_stream_observed_placement")?;
    assert!(
        definition.ends_with("(stream_key, (coalesce(observed_at, deleted_at)) desc) where coalesce(observed_at, deleted_at) is not null"),
        "{definition}"
    );
    Ok(())
}

/// Clause 4.8: the broker stamps `createdAt` and `modifiedAt` on every
/// instance, so every row has a placement on those axes.
#[test]
fn the_created_and_modified_placement_indexes_cover_every_row() -> Result<(), PinError> {
    let schema = schema()?;
    for (name, axis) in [
        ("idx_attribute_history_stream_created_placement", "created_at"),
        ("idx_attribute_history_stream_modified_placement", "modified_at"),
    ] {
        let definition = index_definition(&schema, name)?;
        assert!(
            definition.ends_with(&format!("(stream_key, (coalesce({axis}, deleted_at)) desc)")),
            "{definition}"
        );
        assert!(index(&schema, name)?.predicate().is_none(), "{definition}");
    }
    Ok(())
}

/// Every read on `observedAt` bounds or states the placement beside the
/// column, so the placement index serves it and no index keys the column
/// alone.
#[test]
fn no_index_keys_the_observed_at_column_alone() -> Result<(), PinError> {
    let schema = schema()?;
    let observed: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| index.table() == "attribute_history" && index.method() != "brin")
        .filter(|index| index.referenced_columns().iter().any(|column| column == "observed_at"))
        .map(DeclaredIndex::name)
        .collect();
    pretty_assertions::assert_eq!(observed, ["idx_attribute_history_stream_observed_placement"]);
    Ok(())
}

#[test]
fn attribute_history_declares_no_spatial_index() -> Result<(), PinError> {
    let schema = schema()?;
    let spatial: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| index.table() == "attribute_history" && index.method() == "gist")
        .map(DeclaredIndex::name)
        .collect();
    assert!(spatial.is_empty(), "{spatial:?}");
    Ok(())
}
