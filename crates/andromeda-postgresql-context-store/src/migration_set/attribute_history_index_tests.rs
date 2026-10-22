//! The indexes of `attribute_history`, checked against the embedded
//! migrations.
//!
//! Clause 5.7.4.4 checks a geoquery against the `GeoProperty` instances of each
//! Entity the temporal query selects, so every spatial read of history is one
//! Evolution's rows, reached through the stream indexes. A geometry index over
//! the whole table is only ever ANDed into that lookup, which is why its
//! absence is pinned here.

use crate::migration_set::{
    ddl_text::contains_ci,
    declared_schema::{DeclaredIndex, DeclaredSchema},
};

/// The declared definition of the named `attribute_history` index, its
/// whitespace collapsed.
fn placement_index(schema: &DeclaredSchema, name: &str) -> String {
    let index: &DeclaredIndex = schema
        .indexes
        .iter()
        .find(|index| index.table == "attribute_history" && index.name == name)
        .unwrap_or_else(|| panic!("{name} is declared on attribute_history"));
    index.definition.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Clause 4.11: the in-effect read bounds the placement from above, so a row
/// with neither `observedAt` nor `deletedAt` is never read on that axis.
#[test]
fn the_observed_at_placement_index_skips_rows_placed_nowhere() {
    let schema = DeclaredSchema::of_every_migration();
    let definition = placement_index(&schema, "idx_attribute_history_stream_observed_placement");
    assert!(
        definition.ends_with("(stream_key, (coalesce(observed_at, deleted_at)) desc) where coalesce(observed_at, deleted_at) is not null"),
        "{definition}"
    );
}

/// Clause 4.8: the broker stamps `createdAt` and `modifiedAt` on every
/// instance, so every row has a placement on those axes.
#[test]
fn the_created_and_modified_placement_indexes_cover_every_row() {
    let schema = DeclaredSchema::of_every_migration();
    for (name, axis) in [
        ("idx_attribute_history_stream_created_placement", "created_at"),
        ("idx_attribute_history_stream_modified_placement", "modified_at"),
    ] {
        let definition = placement_index(&schema, name);
        assert!(
            definition.ends_with(&format!("(stream_key, (coalesce({axis}, deleted_at)) desc)")),
            "{definition}"
        );
        assert!(!contains_ci(&definition, " where "), "{definition}");
    }
}

#[test]
fn attribute_history_declares_no_spatial_index() {
    let schema = DeclaredSchema::of_every_migration();
    let spatial: Vec<&str> = schema
        .indexes
        .iter()
        .filter(|index| index.table == "attribute_history" && contains_ci(&index.definition, "USING GIST"))
        .map(|index| index.name.as_str())
        .collect();
    assert!(spatial.is_empty(), "{spatial:?}");
}
