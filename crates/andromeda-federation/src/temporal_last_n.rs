//! Post-aggregation `lastN` truncation of a temporal representation (clauses
//! 5.7.3.4 and 5.7.4.4).
//!
//! `lastN` names the last n timestamps of the temporal property within the
//! concerned interval. Each source applies it to the history it holds, so the
//! merged history of an attribute can carry up to n instances per source; the
//! limit is applied once more over the aggregated instances so the response
//! carries the n most recent across every source.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::cmp::Ordering;

/// The members of a temporal representation that are not attributes.
const ENTITY_MEMBERS: [&str; 8] = ["id", "type", "@context", "scope", "createdAt", "modifiedAt", "deletedAt", "expiresAt"];

/// The `temporalValues` member holding `[value, timestamp]` pairs (clause 4.5.6.2).
const VALUES_MEMBER: &str = "values";

/// Keep, for every attribute of `entity`, the `last_n` instances with the most
/// recent `timeproperty`, in descending order of it.
///
/// Both temporal representations are understood: the normalized one, where an
/// attribute is an array of instances each carrying the temporal property, and
/// the `temporalValues` one, where it is one object whose `values` pairs each
/// end in the timestamp. An attribute in neither shape is left untouched.
pub fn truncate_to_last_n(entity: &mut Value, last_n: u32, timeproperty: TimeProp) {
    let Some(members) = entity.as_object_mut() else {
        return;
    };
    let keep = usize::try_from(last_n).unwrap_or(usize::MAX);
    let property = timeproperty.to_string();
    for (name, value) in members.iter_mut() {
        if ENTITY_MEMBERS.contains(&name.as_str()) {
            continue;
        }
        match value {
            Value::Array(instances) => {
                sort_most_recent_first(instances, |instance| instance.get(&property));
                instances.truncate(keep);
            }
            Value::Object(attribute) => {
                if let Some(Value::Array(pairs)) = attribute.get_mut(VALUES_MEMBER) {
                    sort_most_recent_first(pairs, |pair| pair.as_array().and_then(|pair| pair.last()));
                    pairs.truncate(keep);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

/// Order `items` by the timestamp `instant_of` reads off each, most recent
/// first; items without a readable timestamp sort last, in their given order.
fn sort_most_recent_first(items: &mut [Value], instant_of: impl Fn(&Value) -> Option<&Value>) {
    items.sort_by(|left, right| match (instant(instant_of(left)), instant(instant_of(right))) {
        (Some(left_at), Some(right_at)) => right_at.cmp(&left_at),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

/// The instant a timestamp member names, when it is an RFC 3339 string.
fn instant(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let raw = value?.as_str()?;
    DateTime::parse_from_rfc3339(raw).ok().map(|at| at.with_timezone(&Utc))
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
