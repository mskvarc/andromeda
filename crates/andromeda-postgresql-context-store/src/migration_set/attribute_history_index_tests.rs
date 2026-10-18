//! The indexes of `attribute_history`, checked against the embedded
//! migrations.
//!
//! Clause 5.7.4.4 checks a geoquery against the `GeoProperty` instances of each
//! Entity the temporal query selects, so every spatial read of history is one
//! Evolution's rows, reached through the stream indexes. A geometry index over
//! the whole table is only ever ANDed into that lookup, which is why its
//! absence is pinned here.

use crate::migration_set::{ddl_text::contains_ci, declared_schema::DeclaredSchema};

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
