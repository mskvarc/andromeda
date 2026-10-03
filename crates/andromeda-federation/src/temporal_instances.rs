//! The attribute instances of a merged temporal representation, read as the
//! JSON the local history and every context source answered with.
//!
//! Two representations reach the merge (clause 4.5.9): the normalized one,
//! where an attribute is an array of instances each carrying its Temporal
//! Property, and `temporalValues`, where an attribute is one object -- or, for
//! several `datasetId`s, an array of them -- whose family member holds
//! `[value, instant]` pairs. Both are walked the same way here, so every
//! post-merge step reads an instance's instant alike.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The members of a temporal representation that are not attributes.
const ENTITY_MEMBERS: [&str; 8] = ["id", "type", "@context", "scope", "createdAt", "modifiedAt", "deletedAt", "expiresAt"];

/// The clause 4.5.9 members of a `temporalValues` attribute that hold
/// `[value, instant]` pairs, one per attribute family.
const PAIR_MEMBERS: [&str; 7] = ["values", "objects", "valueLists", "objectLists", "languageMaps", "jsons", "vocabs"];

/// How one element of an attribute's instance list carries its instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceShape {
    /// A normalized instance, naming the instant under the Temporal Property.
    Normalized,
    /// A clause 4.5.9 pair, whose last element is the instant.
    Pair,
}

impl InstanceShape {
    /// The instant `item` carries on `timeproperty`, when it is a readable
    /// RFC 3339 timestamp.
    pub(crate) fn instant(self, item: &Value, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
        let raw = match self {
            InstanceShape::Normalized => item.get(<&'static str>::from(timeproperty)),
            InstanceShape::Pair => item.as_array().and_then(|pair| pair.last()),
        }?;
        DateTime::parse_from_rfc3339(raw.as_str()?).ok().map(|at| at.with_timezone(&Utc))
    }
}

/// How one member of a temporal representation holds its instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberLayout {
    /// Not an attribute, or an attribute holding no instance list.
    NoInstances,
    /// A normalized array of instances.
    Normalized,
    /// One `temporalValues` attribute object.
    PairObject,
    /// One `temporalValues` attribute object per `datasetId`.
    PairObjects,
}

/// How the member `name` holding `value` holds its instances.
fn layout(name: &str, value: &Value) -> MemberLayout {
    if ENTITY_MEMBERS.contains(&name) {
        return MemberLayout::NoInstances;
    }
    match value {
        Value::Array(items) if !items.is_empty() && items.iter().all(holds_pairs) => MemberLayout::PairObjects,
        Value::Array(_) => MemberLayout::Normalized,
        Value::Object(_) => MemberLayout::PairObject,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => MemberLayout::NoInstances,
    }
}

/// Visit every instance list `entity` carries, with the shape its elements
/// have, for editing.
pub(crate) fn for_each_instance_list_mut(entity: &mut Value, mut visit: impl FnMut(&mut Vec<Value>, InstanceShape)) {
    let Some(members) = entity.as_object_mut() else {
        return;
    };
    for (name, value) in members.iter_mut() {
        match (layout(name, value), value) {
            (MemberLayout::Normalized, Value::Array(instances)) => visit(instances, InstanceShape::Normalized),
            (MemberLayout::PairObjects, Value::Array(grouped)) => grouped.iter_mut().for_each(|attribute| pair_lists_mut(attribute, &mut visit)),
            (MemberLayout::PairObject, attribute) => pair_lists_mut(attribute, &mut visit),
            (MemberLayout::NoInstances | MemberLayout::Normalized | MemberLayout::PairObjects, _) => {}
        }
    }
}

/// Visit every instance list `entity` carries, with the shape its elements
/// have.
fn for_each_instance_list(entity: &Value, mut visit: impl FnMut(&[Value], InstanceShape)) {
    let Some(members) = entity.as_object() else {
        return;
    };
    for (name, value) in members {
        match (layout(name, value), value) {
            (MemberLayout::Normalized, Value::Array(instances)) => visit(instances, InstanceShape::Normalized),
            (MemberLayout::PairObjects, Value::Array(grouped)) => grouped.iter().for_each(|attribute| pair_lists(attribute, &mut visit)),
            (MemberLayout::PairObject, attribute) => pair_lists(attribute, &mut visit),
            (MemberLayout::NoInstances | MemberLayout::Normalized | MemberLayout::PairObjects, _) => {}
        }
    }
}

/// How many attribute instances `entity` carries, a `temporalValues` pair
/// counting as the instance it stands for (clause 4.5.9).
pub(crate) fn instance_count(entity: &Value) -> usize {
    let mut count = 0_usize;
    for_each_instance_list(entity, |items, _| count = count.saturating_add(items.len()));
    count
}

/// Keep, in every instance list of `entity`, only the instances whose instant
/// on `timeproperty` `keep` admits; an instance with no readable instant is
/// kept, since nothing places it outside.
pub fn retain_instances(entity: &mut Value, timeproperty: TimeProp, keep: impl Fn(DateTime<Utc>) -> bool) {
    for_each_instance_list_mut(entity, |items, shape| {
        items.retain(|item| shape.instant(item, timeproperty).is_none_or(&keep));
    });
}

/// The least and most recent instants on `timeproperty` any of `entities`
/// carries, or `None` when none carries one.
#[must_use]
pub fn instant_span(entities: &[Value], timeproperty: TimeProp) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let mut span: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    for entity in entities {
        for_each_instance_list(entity, |items, shape| {
            for at in items.iter().filter_map(|item| shape.instant(item, timeproperty)) {
                span = Some(span.map_or((at, at), |(least, most)| (least.min(at), most.max(at))));
            }
        });
    }
    span
}

/// Visit the pair members of one `temporalValues` attribute object, for editing.
fn pair_lists_mut(attribute: &mut Value, visit: &mut impl FnMut(&mut Vec<Value>, InstanceShape)) {
    let Some(members) = attribute.as_object_mut() else {
        return;
    };
    for member in PAIR_MEMBERS {
        if let Some(Value::Array(pairs)) = members.get_mut(member) {
            visit(pairs, InstanceShape::Pair);
        }
    }
}

/// Visit the pair members of one `temporalValues` attribute object.
fn pair_lists(attribute: &Value, visit: &mut impl FnMut(&[Value], InstanceShape)) {
    for member in PAIR_MEMBERS {
        if let Some(Value::Array(pairs)) = attribute.get(member) {
            visit(pairs, InstanceShape::Pair);
        }
    }
}

/// Whether `item` is a `temporalValues` attribute object rather than a
/// normalized instance.
fn holds_pairs(item: &Value) -> bool {
    pair_member(item).is_some()
}

/// The clause 4.5.9 member a `temporalValues` attribute object holds its
/// pairs in; `None` for a normalized instance.
pub(crate) fn pair_member(item: &Value) -> Option<&'static str> {
    let members = item.as_object()?;
    PAIR_MEMBERS.into_iter().find(|member| members.contains_key(*member))
}

#[cfg(test)]
mod tests {
    use crate::temporal_instances::{instant_span, retain_instances};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    fn observed(value: i64, observed_at: &str) -> Value {
        json!({"type": "Property", "value": value, "observedAt": observed_at})
    }

    /// Normalized instances are kept or dropped by the instant under the
    /// Temporal Property, and every other member passes untouched.
    #[test]
    fn normalized_instances_are_retained_by_their_instant() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "createdAt": "2000-01-01T00:00:00Z",
            "speed": [observed(1, "2020-01-01T00:00:00Z"), observed(2, "2020-01-02T00:00:00Z"), observed(3, "2020-01-03T00:00:00Z")]
        });
        retain_instances(&mut entity, TimeProp::Observed, |instant| instant <= at("2020-01-02T00:00:00Z"));
        pretty_assertions::assert_eq!(
            entity["speed"],
            json!([observed(1, "2020-01-01T00:00:00Z"), observed(2, "2020-01-02T00:00:00Z")])
        );
        pretty_assertions::assert_eq!(entity["createdAt"], json!("2000-01-01T00:00:00Z"));
    }

    /// Clause 4.5.9: every family member of a `temporalValues` attribute, and
    /// every `datasetId` group of one, is read by the instant ending each pair.
    #[test]
    fn temporal_values_pairs_of_every_family_and_dataset_are_retained_by_their_instant() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "values": [[1, "2020-01-01T00:00:00Z"], [2, "2020-01-03T00:00:00Z"]]},
            "parkedIn": {"type": "Relationship", "objects": [["urn:a", "2020-01-01T00:00:00Z"], ["urn:b", "2020-01-03T00:00:00Z"]]},
            "fuel": [
                {"type": "Property", "datasetId": "urn:ds:1", "values": [[5, "2020-01-01T00:00:00Z"]]},
                {"type": "Property", "datasetId": "urn:ds:2", "values": [[6, "2020-01-03T00:00:00Z"]]}
            ]
        });
        retain_instances(&mut entity, TimeProp::Observed, |instant| instant < at("2020-01-02T00:00:00Z"));
        pretty_assertions::assert_eq!(entity["speed"]["values"], json!([[1, "2020-01-01T00:00:00Z"]]));
        pretty_assertions::assert_eq!(entity["parkedIn"]["objects"], json!([["urn:a", "2020-01-01T00:00:00Z"]]));
        pretty_assertions::assert_eq!(entity["fuel"][0]["values"], json!([[5, "2020-01-01T00:00:00Z"]]));
        pretty_assertions::assert_eq!(entity["fuel"][1]["values"], json!([]));
    }

    /// An instance with no readable instant has nothing placing it outside,
    /// so it stays.
    #[test]
    fn an_instance_without_a_readable_instant_is_kept() {
        let mut entity = json!({"id": "urn:x", "type": "T", "speed": [{"type": "Property", "value": 1}]});
        retain_instances(&mut entity, TimeProp::Observed, |_| false);
        pretty_assertions::assert_eq!(entity["speed"].as_array().map(Vec::len), Some(1));
    }

    /// The span is the least and most recent instant across every entity and
    /// representation; entities carrying none contribute nothing.
    #[test]
    fn the_span_covers_every_instance_of_every_entity() {
        let entities = vec![
            json!({"id": "urn:a", "type": "T", "speed": [observed(1, "2020-01-02T00:00:00Z")]}),
            json!({"id": "urn:b", "type": "T", "speed": {"type": "Property", "values": [[1, "2020-01-01T00:00:00Z"], [2, "2020-01-05T00:00:00Z"]]}}),
            json!({"id": "urn:c", "type": "T", "speed": []}),
        ];
        pretty_assertions::assert_eq!(
            instant_span(&entities, TimeProp::Observed),
            Some((at("2020-01-01T00:00:00Z"), at("2020-01-05T00:00:00Z")))
        );
        pretty_assertions::assert_eq!(instant_span(&entities[2..], TimeProp::Observed), None);
    }
}
