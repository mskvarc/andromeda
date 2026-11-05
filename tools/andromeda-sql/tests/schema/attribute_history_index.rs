//! The indexes of `attribute_history`.
//!
//! Clause 5.7.4.4 checks a geoquery against the `GeoProperty` instances of each
//! Entity the temporal query selects, so every spatial read of history is one
//! Evolution's rows, reached through the stream indexes. A geometry index over
//! the whole table is only ever intersected with that lookup, which is why its
//! absence is pinned here.

use crate::repository::{PinError, index, index_definition, schema};
use andromeda_sql::declared_schema::index::DeclaredIndex;

/// Every read of a stream on a time axis is a range of the rows carrying it,
/// ordered on the axis and then on the write that recorded the row, so each
/// axis's stream-led index holds only those rows. The `observed_at` and
/// `deleted_at` indexes key that whole order, descending as a `lastN` reads
/// it; the `created_at` and `modified_at` indexes key the write instant
/// alone, which every instance of one write shares, so B-tree deduplication
/// folds those ties into one entry, and ascend, so a full leaf splits at the
/// stream's newest entry and keeps its older entries on a full page.
#[test]
fn each_time_axis_has_a_stream_index_over_the_rows_carrying_it() -> Result<(), PinError> {
    let schema = schema()?;
    for (axis, key) in [
        ("observed_at", "observed_at desc, write_id desc, write_ordinal desc"),
        ("created_at", "created_at"),
        ("modified_at", "modified_at"),
        ("deleted_at", "deleted_at desc, write_id desc, write_ordinal desc"),
    ] {
        let name = format!("idx_attribute_history_stream_{axis}");
        pretty_assertions::assert_eq!(index(&schema, &name)?.table(), "attribute_history");
        let definition = index_definition(&schema, &name)?;
        assert!(definition.ends_with(&format!("(stream_key, {key}) where {axis} is not null")), "{definition}");
    }
    Ok(())
}

/// No B-tree keys an axis through another: a row is read on an axis by its
/// own column alone, so every B-tree naming `observed_at` is its stream index.
#[test]
fn only_the_observed_at_stream_index_keys_the_observed_at_column() -> Result<(), PinError> {
    let schema = schema()?;
    let observed: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| index.table() == "attribute_history" && index.method() != "brin")
        .filter(|index| index.referenced_columns().iter().any(|column| column == "observed_at"))
        .map(DeclaredIndex::name)
        .collect();
    pretty_assertions::assert_eq!(observed, ["idx_attribute_history_stream_observed_at"]);
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
