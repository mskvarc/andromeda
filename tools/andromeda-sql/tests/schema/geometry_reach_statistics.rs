//! The statistics the planner prices a geo-query's box test from.
//!
//! Every geo-query tests the box a spatial index keys, a geometry's
//! great-circle reach where it has one, before its exact relation. Each such
//! index is partial, and the planner reads no statistics from the expression
//! of a partial index, so the box is priced from an expression statistics
//! object on the same expression.

use crate::repository::{PinError, schema, written};
use pretty_assertions::assert_eq;

/// Every partial `GiST` index keyed by a reach, as `(table, reach box)`, each
/// matched by a statistics object over the same expression.
#[test]
fn every_reach_box_an_index_keys_has_expression_statistics() -> Result<(), PinError> {
    let schema = schema()?;
    let boxes: Vec<(&str, &str)> = schema
        .indexes()
        .iter()
        .filter(|index| index.method() == "gist" && index.predicate().is_some())
        .flat_map(|index| index.elements().iter().map(move |element| (index.table(), element.written())))
        .filter(|(_, key)| key.starts_with("coalesce(") && key.contains("_reach,"))
        .collect();
    assert_eq!(boxes.len(), 5, "{boxes:?}");
    for (table, key) in boxes {
        let expected = format!("on ({key}) from {table}");
        assert!(
            schema
                .statistics()
                .iter()
                .any(|statistics| statistics.table() == table && written(&schema, statistics.origin(), statistics.definition()) == expected),
            "no statistics object `{expected}`"
        );
    }
    Ok(())
}
