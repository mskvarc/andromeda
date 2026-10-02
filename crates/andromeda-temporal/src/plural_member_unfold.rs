//! Unfolding the plural members of a written Temporal Evolution (clause
//! 4.5.9) into the instances they stand for.
//!
//! A plural member (`values`, `objects`, `languageMaps`, `vocabs`, `jsons`,
//! `valueLists`, `objectLists`) carries several observations under one
//! envelope, each inheriting the envelope's members unless it restates them.
//! Unfolding copies the inherited members into every observation, so the
//! copies, not the request body, decide what the unfold costs: a small body
//! whose envelope carries a large sub-attribute and whose plural member holds
//! many empty observations unfolds into a payload many times its size. The
//! copies are therefore counted before any is made, every inherited value once
//! per observation inheriting it, against the entity value limit the written
//! document is held to (clause 6.3.2, "Request Entity too large"). An
//! observation's own members are not counted here: they are moved, not
//! copied, and the request value budget already bounded them.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
use serde_json::{Map, Value};
use std::{collections::HashMap, slice::from_ref};

/// Unfold every plural member of `payload` into the instances it stands for,
/// once the values its observations inherit fit `limit`.
///
/// An instance holding its family's plural member becomes one instance per
/// element: the envelope's members without the plural one, overlaid by the
/// element's own, so a member an element restates overrides the envelope's.
///
/// # Errors
///
/// Returns [`PayloadBudgetExceeded::InheritedObservationValues`] when the
/// observations inherit more values than `limit` admits, before anything is
/// unfolded.
pub(crate) fn unfold_plural_members(payload: &mut Value, limit: JsonValueLimit) -> Result<(), PayloadBudgetExceeded> {
    let Some(members) = payload.as_object_mut() else {
        return Ok(());
    };
    admit_inherited_values(members, limit)?;
    for member in members.values_mut() {
        if !instances(member)
            .iter()
            .any(|instance| instance.as_object().is_some_and(|instance| plural_key(instance).is_some()))
        {
            continue;
        }
        let instances = match member.take() {
            Value::Array(instances) => instances,
            single @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => vec![single],
        };
        *member = Value::Array(instances.into_iter().flat_map(unfold_instance).collect());
    }
    Ok(())
}

/// Refuse a payload whose observations inherit more values than `limit`
/// admits, counted over every instance of every attribute.
fn admit_inherited_values(members: &Map<String, Value>, limit: JsonValueLimit) -> Result<(), PayloadBudgetExceeded> {
    members
        .values()
        .flat_map(instances)
        .filter_map(Value::as_object)
        .try_fold(0_usize, |counted, envelope| {
            inherited_values(envelope)
                .and_then(|inherited| counted.checked_add(inherited))
                .filter(|total| *total <= limit.get())
                .ok_or(PayloadBudgetExceeded::InheritedObservationValues { limit })
        })
        .map(drop)
}

/// The values the observations of one instance inherit from it: for every
/// observation, the copied envelope object and every value of each envelope
/// member the observation does not restate; `None` past `usize`.
///
/// Each member is counted once and each observation reads only its own
/// members, so the count costs what the payload holds, not what the unfold
/// would build.
fn inherited_values(envelope: &Map<String, Value>) -> Option<usize> {
    let Some(key) = plural_key(envelope) else {
        return Some(0);
    };
    let Some(Value::Array(observations)) = envelope.get(key) else {
        return Some(0);
    };
    let members: HashMap<&str, usize> = envelope
        .iter()
        .filter(|(name, _)| name.as_str() != key)
        .map(|(name, value)| (name.as_str(), value_count(value)))
        .collect();
    let whole = members.values().try_fold(1_usize, |sum, values| sum.checked_add(*values))?;
    observations.iter().try_fold(0_usize, |sum, observation| {
        let restated: usize = observation
            .as_object()
            .map_or(0, |own| own.keys().filter_map(|name| members.get(name.as_str())).sum());
        sum.checked_add(whole.saturating_sub(restated))
    })
}

/// The JSON values `value` holds, itself included.
fn value_count(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.iter().map(value_count).fold(1, usize::saturating_add),
        Value::Object(members) => members.values().map(value_count).fold(1, usize::saturating_add),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

/// The instances an attribute member holds: one instance object, or an array
/// of them.
fn instances(member: &Value) -> &[Value] {
    match member {
        Value::Array(instances) => instances,
        Value::Object(_) => from_ref(member),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => &[],
    }
}

/// The plural member an instance's declared family holds its observations in,
/// when the instance holds it.
fn plural_key(instance: &Map<String, Value>) -> Option<&'static str> {
    let family = instance.get("type").and_then(Value::as_str)?.parse::<AttributeType>().ok()?;
    let key = family.temporal_values_key();
    instance.contains_key(key).then_some(key)
}

/// The instances one instance stands for: itself, or one per element of its
/// plural member.
fn unfold_instance(instance: Value) -> Vec<Value> {
    let Value::Object(mut envelope) = instance else {
        return vec![instance];
    };
    let Some(key) = plural_key(&envelope) else {
        return vec![Value::Object(envelope)];
    };
    match envelope.remove(key) {
        Some(Value::Array(observations)) => observations
            .into_iter()
            .map(|observation| {
                let mut unfolded = envelope.clone(); // clone: every observation inherits the envelope's members, counted before the unfold
                if let Value::Object(own) = observation {
                    unfolded.extend(own);
                }
                Value::Object(unfolded)
            })
            .collect(),
        Some(other) => {
            envelope.insert(key.to_owned(), other);
            vec![Value::Object(envelope)]
        }
        None => vec![Value::Object(envelope)],
    }
}

#[cfg(test)]
mod tests {
    use crate::plural_member_unfold::unfold_plural_members;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// A limit of `values` JSON values.
    fn limit(values: usize) -> JsonValueLimit {
        JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive test limit"))
    }

    /// A limit no test payload reaches.
    fn unbounded() -> JsonValueLimit {
        limit(usize::MAX)
    }

    /// A `speed` envelope whose `meta` sub-attribute holds ten numbers, over
    /// `count` copies of `observation`.
    ///
    /// Each observation that restates nothing inherits fifteen values: the
    /// copied instance object, its `type`, and `meta` (the object, its `type`,
    /// the `value` array and its ten numbers).
    fn envelope(observation: &Value, count: usize) -> Value {
        json!({
            "type": "Property",
            "meta": {"type": "Property", "value": (0..10).collect::<Vec<u32>>()},
            "values": vec![observation; count]
        })
    }

    /// Whether `payload` unfolds at exactly `values` inherited values and is
    /// refused, untouched, one below.
    fn inherits_exactly(payload: &Value, values: usize) -> bool {
        let mut refused = payload.clone(); // clone: the payload is unfolded under two limits
        let refusal = unfold_plural_members(&mut refused, limit(values - 1));
        let mut admitted = payload.clone(); // clone: the payload is unfolded under two limits
        refusal == Err(PayloadBudgetExceeded::InheritedObservationValues { limit: limit(values - 1) })
            && refused == *payload
            && unfold_plural_members(&mut admitted, limit(values)).is_ok()
    }

    /// Clause 4.5.9: each element of a plural member is its own observation,
    /// inheriting the envelope's members and overriding the ones it restates.
    #[test]
    fn a_plural_member_unfolds_into_one_instance_per_element() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "unitCode": "KMH",
                "datasetId": "urn:ngsi-ld:Dataset:sensor",
                "values": [
                    {"value": 55, "observedAt": "2021-01-01T00:00:00Z"},
                    {"value": 61, "observedAt": "2021-01-02T00:00:00Z", "unitCode": "MTS"}
                ]
            },
            "brand": {"type": "Property", "value": "Mercedes"}
        });
        unfold_plural_members(&mut payload, unbounded()).expect("within the limit");
        pretty_assertions::assert_eq!(
            payload,
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "unitCode": "KMH", "datasetId": "urn:ngsi-ld:Dataset:sensor", "value": 55, "observedAt": "2021-01-01T00:00:00Z"},
                    {"type": "Property", "unitCode": "MTS", "datasetId": "urn:ngsi-ld:Dataset:sensor", "value": 61, "observedAt": "2021-01-02T00:00:00Z"}
                ],
                "brand": {"type": "Property", "value": "Mercedes"}
            })
        );
    }

    /// The two list families hold their observations under their own plural
    /// members (clause 4.5.9), which unfold the same way.
    #[test]
    fn a_list_family_unfolds_its_own_plural_member() {
        let mut payload = json!({
            "parts": [{"type": "ListRelationship", "objectLists": [{"objectList": [{"object": "urn:ngsi-ld:Part:1"}]}]}]
        });
        unfold_plural_members(&mut payload, unbounded()).expect("within the limit");
        pretty_assertions::assert_eq!(
            payload,
            json!({"parts": [{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Part:1"}]}]})
        );
    }

    /// Every observation copies what it inherits, so five observations that
    /// restate nothing inherit five times the envelope's fifteen values.
    #[test]
    fn every_observation_counts_the_values_it_inherits() {
        let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": envelope(&json!({}), 5)});
        assert!(inherits_exactly(&payload, 75), "{payload}");
    }

    /// A member an observation restates is its own, not a copy: only the
    /// copied object and `type` remain inherited, two values each.
    #[test]
    fn a_member_an_observation_restates_is_not_inherited() {
        let restating = json!({"meta": {"type": "Property", "value": 1}});
        let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": envelope(&restating, 5)});
        assert!(inherits_exactly(&payload, 10), "{payload}");
    }

    /// The count spans every instance of every attribute: an instance array
    /// and a second attribute add to the same total.
    #[test]
    fn the_inherited_values_of_every_instance_add_up() {
        let mut second = envelope(&json!({}), 5);
        second["datasetId"] = json!("urn:ngsi-ld:Dataset:b");
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            // The second instance inherits its datasetId as well: 16 values each.
            "speed": [envelope(&json!({}), 5), second],
            "heading": envelope(&json!({}), 5)
        });
        assert!(inherits_exactly(&payload, 75 + 80 + 75), "{payload}");
    }

    /// An instance without a plural member inherits nothing to anything.
    #[test]
    fn an_instance_without_a_plural_member_inherits_nothing() {
        let mut payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": (0..100).collect::<Vec<u32>>()}});
        let written = payload.clone(); // clone: the payload is compared after the unfold
        unfold_plural_members(&mut payload, limit(1)).expect("nothing is inherited");
        pretty_assertions::assert_eq!(payload, written);
    }

    /// A large envelope over many empty observations would unfold into far
    /// more than the body holds; it is refused before a single copy is made.
    #[test]
    fn a_large_envelope_over_many_observations_is_refused_before_it_is_copied() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "meta": {"type": "Property", "value": (0..1_000).collect::<Vec<u32>>()},
                "values": vec![json!({}); 100_000]
            }
        });
        let refusal = unfold_plural_members(&mut payload, limit(1_000_000)).expect_err("about 100 million inherited values");
        pretty_assertions::assert_eq!(refusal, PayloadBudgetExceeded::InheritedObservationValues { limit: limit(1_000_000) });
        assert!(payload["speed"]["values"].is_array(), "nothing was unfolded");
    }
}
