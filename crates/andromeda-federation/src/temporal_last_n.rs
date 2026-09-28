//! Post-aggregation `lastN` truncation of a temporal representation (clauses
//! 5.7.3.4 and 5.7.4.4).
//!
//! `lastN` names the last n timestamps of the temporal property within the
//! concerned interval. Each source applies it to the history it holds, so the
//! merged history of an attribute can carry up to n instances per source; the
//! limit is applied once more over the aggregated instances so the response
//! carries the n most recent across every source.

use crate::temporal_instances::{InstanceShape, for_each_instance_list_mut};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use serde_json::Value;
use std::cmp::Ordering;

/// Keep, for every attribute of `entity`, the `last_n` instances with the most
/// recent `timeproperty`, in descending order of it.
///
/// Both temporal representations are understood: the normalized one, where an
/// attribute is an array of instances each carrying the temporal property, and
/// the `temporalValues` one, whose family member holds pairs each ending in the
/// timestamp -- one list per `datasetId` group.
pub fn truncate_to_last_n(entity: &mut Value, last_n: u32, timeproperty: TimeProp) {
    let keep = usize::try_from(last_n).unwrap_or(usize::MAX);
    for_each_instance_list_mut(entity, |items, shape| {
        sort_most_recent_first(items, shape, timeproperty);
        items.truncate(keep);
    });
}

/// Order `items` by their instant on `timeproperty`, most recent first; items
/// without a readable instant sort last, in their given order.
fn sort_most_recent_first(items: &mut [Value], shape: InstanceShape, timeproperty: TimeProp) {
    items.sort_by(|left, right| match (shape.instant(left, timeproperty), shape.instant(right, timeproperty)) {
        (Some(left_at), Some(right_at)) => right_at.cmp(&left_at),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

#[cfg(test)]
mod tests {
    use crate::temporal_last_n::truncate_to_last_n;
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use serde_json::{Value, json};

    fn observed(value: i64, observed_at: &str) -> Value {
        json!({"type": "Property", "value": value, "observedAt": observed_at})
    }

    /// Clause 5.7.4.4: the two most recent instances across the merged history
    /// are kept, whichever source they came from and in whatever order they
    /// were merged.
    #[test]
    fn the_most_recent_instances_across_the_merged_history_are_kept() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                observed(10, "2020-08-01T12:00:00Z"),
                observed(50, "2020-08-01T12:04:00Z"),
                observed(20, "2020-08-01T12:01:00Z"),
                observed(40, "2020-08-01T12:03:00Z"),
                observed(30, "2020-08-01T12:02:00Z")
            ]
        });
        truncate_to_last_n(&mut entity, 2, TimeProp::Observed);
        pretty_assertions::assert_eq!(
            entity["speed"],
            json!([observed(50, "2020-08-01T12:04:00Z"), observed(40, "2020-08-01T12:03:00Z")])
        );
        pretty_assertions::assert_eq!(entity["type"], json!("Vehicle"));
    }

    /// The time property the query named is the one the truncation reads, and
    /// an attribute with fewer instances than `lastN` keeps them all.
    #[test]
    fn the_named_time_property_drives_the_truncation() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z", "modifiedAt": "2021-03-01T00:00:00Z"},
                {"type": "Property", "value": 2, "observedAt": "2020-02-01T00:00:00Z", "modifiedAt": "2021-01-01T00:00:00Z"}
            ],
            "fuel": [observed(7, "2020-01-01T00:00:00Z")]
        });
        truncate_to_last_n(&mut entity, 1, TimeProp::Modified);
        pretty_assertions::assert_eq!(entity["speed"][0]["value"], json!(1));
        pretty_assertions::assert_eq!(entity["fuel"].as_array().map(Vec::len), Some(1));
    }

    /// The `temporalValues` representation keeps its `[value, timestamp]`
    /// pairs, truncated by the timestamp that ends each pair.
    #[test]
    fn temporal_values_pairs_are_truncated_by_their_timestamp() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "values": [[10, "2020-08-01T12:00:00Z"], [30, "2020-08-01T12:02:00Z"], [20, "2020-08-01T12:01:00Z"]]}
        });
        truncate_to_last_n(&mut entity, 2, TimeProp::Observed);
        pretty_assertions::assert_eq!(entity["speed"]["values"], json!([[30, "2020-08-01T12:02:00Z"], [20, "2020-08-01T12:01:00Z"]]));
    }
}
