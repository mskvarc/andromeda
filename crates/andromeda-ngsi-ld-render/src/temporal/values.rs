//! Temporal values renderer (NGSI-LD clause 4.5.9).
//!
//! [`TemporalValuesRenderer`] transforms a compacted temporal entity into the
//! `temporalValues` simplified representation.

use crate::object_list::bare_uri_object_list;
use andromeda_ngsi_ld_lexicon::{
    attribute_type::{AttributeType, TemporalValueEncoding},
    time_prop::TimeProp,
    uri::Uri,
};
use indexmap::IndexMap;
use serde_json::{Value, json};
use thiserror::Error;

/// The entity-level members that pass through untouched: clause 4.5.9 collapses
/// attributes alone, so the identity and the clause 4.8 system timestamps are
/// copied across rather than read as instance arrays.
const RESERVED: &[&str] = &["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

/// A compacted temporal entity contains a malformed typed member.
#[derive(Debug, Error)]
pub enum TemporalValuesError {
    /// A `datasetId` is not a URI as required by clause 4.5.5.
    #[error("temporal value contains an invalid datasetId URI")]
    InvalidDatasetId,
    /// An attribute instance has an unknown NGSI-LD attribute type.
    #[error("temporal value contains an invalid attribute type")]
    InvalidAttributeType,
}

/// Transforms a compacted temporal entity into `temporalValues` form
/// (NGSI-LD clause 4.5.9).
///
/// Each attribute is collapsed into `[value, instant]` pairs grouped by
/// dataset ID, the instant being the one the query placed the instance by.
/// Reserved entity-level keys (`id`, `type`, `@context`, timestamps) pass
/// through unchanged.
#[derive(Debug)]
pub struct TemporalValuesRenderer {
    /// Compacted temporal entity to project.
    payload: Value,
    /// The Temporal Property the query ran on (clause 4.11), whose instant is
    /// the second element of every pair.
    timeproperty: TimeProp,
}

impl TemporalValuesRenderer {
    /// Create a renderer for the given compacted payload, pairing each value
    /// with the instance's instant on `timeproperty`.
    #[must_use]
    pub const fn new(payload: Value, timeproperty: TimeProp) -> TemporalValuesRenderer {
        TemporalValuesRenderer { payload, timeproperty }
    }

    /// Collapse each attribute into the clause 4.5.9 `temporalValues` pairs.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalValuesError::InvalidDatasetId`] when an instance
    /// carries a `datasetId` that is not a URI, and
    /// [`TemporalValuesError::InvalidAttributeType`] when an instance carries a
    /// `type` that names no NGSI-LD attribute type.
    pub fn render(self) -> Result<Value, TemporalValuesError> {
        let axis: &'static str = self.timeproperty.into();
        let obj = match self.payload {
            Value::Object(m) => m,
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => return Ok(other),
        };

        let mut result = serde_json::Map::new();

        for key in RESERVED {
            if let Some(v) = obj.get(*key) {
                result.insert(key.to_string(), v.clone());
            }
        }

        for (key, value) in &obj {
            if RESERVED.contains(&key.as_str()) {
                continue;
            }

            let instances = match value {
                Value::Array(arr) => arr,
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => {
                    result.insert(key.clone(), other.clone());
                    continue;
                }
            };

            if instances.is_empty() {
                continue;
            }

            let mut groups: IndexMap<Option<Uri>, Vec<&Value>> = IndexMap::new();
            for inst in instances {
                let dataset_id = inst
                    .get("datasetId")
                    .and_then(Value::as_str)
                    .map(|raw| raw.parse::<Uri>().map_err(|_error| TemporalValuesError::InvalidDatasetId))
                    .transpose()?;
                groups.entry(dataset_id).or_default().push(inst);
            }

            let mut tv_objects: Vec<Value> = Vec::new();

            for (dataset_id, insts) in &groups {
                let first = insts[0];
                let attr_type = first
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|raw| raw.parse::<AttributeType>().map_err(|_error| TemporalValuesError::InvalidAttributeType))
                    .transpose()?
                    .unwrap_or(AttributeType::Property);
                let value_key = attr_type.temporal_values_key();
                let extract_key = attr_type.compact_value_key();

                let mut pairs: Vec<Value> = Vec::new();
                for inst in insts {
                    let val = inst.get(extract_key).cloned().unwrap_or(Value::Null);
                    let val = simplified_value(val, attr_type, extract_key);
                    // Clause 4.5.9: "the second element shall correspond to the
                    // associated Temporal Property", which is the one the query
                    // placed the instance by (clause 4.11, `observedAt` by
                    // default). A deletion carries no instant on an observation
                    // axis, so its `deletedAt` stands in for it.
                    let instant = inst.get(axis).or_else(|| inst.get("deletedAt")).cloned().unwrap_or(Value::Null);
                    pairs.push(json!([val, instant]));
                }

                let mut tv_obj = serde_json::Map::new();
                tv_obj.insert("type".to_string(), json!(attr_type));
                if let Some(ds_id) = dataset_id {
                    tv_obj.insert("datasetId".to_string(), json!(ds_id));
                }
                tv_obj.insert(value_key.to_string(), Value::Array(pairs));
                tv_objects.push(Value::Object(tv_obj));
            }

            if tv_objects.len() == 1 {
                if let Some(single) = tv_objects.into_iter().next() {
                    result.insert(key.clone(), single);
                }
            } else {
                result.insert(key.clone(), Value::Array(tv_objects));
            }
        }

        Ok(Value::Object(result))
    }
}

/// The value one clause 4.5.9 pair carries, read out of the normalized
/// instance the projection runs over.
///
/// Three families wrap their value in a one-member object (clause 4.5.9's
/// `languageMaps`, `jsons` and `vocabs`); the rest carry it bare. A
/// `ListRelationship` is the one family whose bare value is not the member it
/// was read from: clause 4.5.9 gives its `objectLists` pairs "an ordered array
/// of Relationship objects (URIs)" — EXAMPLE 7 spells them as bare strings —
/// while the normalized `objectList` those are read from holds the
/// `{"object": …}` members clause 4.5.22.2 mandates.
fn simplified_value(value: Value, attr_type: AttributeType, extract_key: &str) -> Value {
    match attr_type.temporal_value_encoding() {
        TemporalValueEncoding::Wrapped => json!({extract_key: value}),
        TemporalValueEncoding::Bare => match attr_type {
            AttributeType::ListRelationship => bare_uri_object_list(value),
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::Relationship
            | AttributeType::ListProperty
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::VocabProperty => value,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(input: Value) -> Value {
        TemporalValuesRenderer::new(input, TimeProp::Observed)
            .render()
            .expect("valid temporal values payload")
    }

    #[test]
    fn temporal_values_single_property() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"},
                {"type": "Property", "value": 30, "observedAt": "2025-01-01T01:00:00Z"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["speed"]["type"], "Property");
        let values = result["speed"]["values"].as_array().expect("values array");
        pretty_assertions::assert_eq!(values.len(), 2);
        pretty_assertions::assert_eq!(values[0][0], 25);
        pretty_assertions::assert_eq!(values[1][0], 30);
    }

    #[test]
    fn temporal_values_multi_dataset() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 25, "observedAt": "t1", "datasetId": "urn:ds:1"},
                {"type": "Property", "value": 30, "observedAt": "t2", "datasetId": "urn:ds:2"}
            ]
        });
        let result = transform(input);
        let arr = result["speed"].as_array().expect("multi-dataset array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }

    #[test]
    fn temporal_values_preserves_reserved() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "@context": "https://example.org/ctx",
            "speed": [{"type": "Property", "value": 10, "observedAt": "t1"}]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["id"], "urn:x");
        pretty_assertions::assert_eq!(result["type"], "Vehicle");
        pretty_assertions::assert_eq!(result["@context"], "https://example.org/ctx");
    }

    #[test]
    fn temporal_values_relationship() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "isParked": [
                {"type": "Relationship", "object": "urn:p1", "observedAt": "t1"},
                {"type": "Relationship", "object": "urn:p2", "observedAt": "t2"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["isParked"]["type"], "Relationship");
        let objects = result["isParked"]["objects"].as_array().expect("objects array");
        pretty_assertions::assert_eq!(objects.len(), 2);
        pretty_assertions::assert_eq!(objects[0][0], "urn:p1");
    }

    /// Clause 4.5.9: "For each `ListProperty` ... shall only contain a member
    /// whose key shall be `valueLists` ... Each array element shall be another
    /// array containing exactly two elements: the first element shall be an
    /// ordered array of Property values, and the second element shall
    /// correspond to the associated Temporal Property".
    #[test]
    fn a_list_property_collapses_into_value_lists_of_ordered_arrays() {
        let input = json!({
            "id": "urn:x",
            "type": "Building",
            "period": [
                {"type": "ListProperty", "valueList": ["First", "Second"], "observedAt": "2022-08-09T18:25:02Z"},
                {"type": "ListProperty", "valueList": ["1st", "2nd"], "observedAt": "2022-08-10T18:25:02Z"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["period"]["type"], "ListProperty");
        pretty_assertions::assert_eq!(
            result["period"]["valueLists"],
            json!([[["First", "Second"], "2022-08-09T18:25:02Z"], [["1st", "2nd"], "2022-08-10T18:25:02Z"]])
        );
        // Clause 4.5.21.2 prohibits `value` on a ListProperty, and clause 4.5.9
        // gives the family one member alone, so the generalized `values` the
        // Property family collapses into is absent as well.
        assert!(result["period"].get("values").is_none(), "{}", result["period"]);
        assert!(result["period"].get("value").is_none(), "{}", result["period"]);
    }

    /// Clause 4.5.9: for each `ListRelationship` the member is `objectLists`,
    /// whose pairs hold "an ordered array of Relationship objects (URIs)" and
    /// the associated Temporal Property.
    #[test]
    fn a_list_relationship_collapses_into_object_lists_of_ordered_uris() {
        let input = json!({
            "id": "urn:x",
            "type": "Building",
            "membersPresent": [
                {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], "observedAt": "2022-08-09T18:25:02Z"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["membersPresent"]["type"], "ListRelationship");
        pretty_assertions::assert_eq!(
            result["membersPresent"]["objectLists"],
            json!([[["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], "2022-08-09T18:25:02Z"]])
        );
        // Clause 4.5.22.2 prohibits `object` on a ListRelationship, so neither
        // it nor the `objects` the Relationship family collapses into appears.
        assert!(result["membersPresent"].get("objects").is_none(), "{}", result["membersPresent"]);
        assert!(result["membersPresent"].get("object").is_none(), "{}", result["membersPresent"]);
    }

    /// Clause 4.5.9, `ListRelationship` bullet: "the first element shall be an
    /// ordered array of Relationship objects (URIs)", and EXAMPLE 7 spells them
    /// as bare strings. The normalized instance this is projected from holds the
    /// `{"object": …}` members clause 4.5.22.2 mandates, so the projection reads
    /// each member's target back out rather than carrying the member itself.
    #[test]
    fn a_normalized_object_list_collapses_into_the_bare_uris_clause_4_5_9_names() {
        let input = json!({
            "id": "urn:x",
            "type": "Building",
            "membersPresent": [{
                "type": "ListRelationship",
                "objectList": [{"object": "urn:ngsi-ld:Person:Alice"}, {"object": "urn:ngsi-ld:Person:Bob"}],
                "observedAt": "2022-08-09T18:25:02Z"
            }]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(
            result["membersPresent"]["objectLists"],
            json!([[["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], "2022-08-09T18:25:02Z"]])
        );
    }

    /// Clause 4.5.9, Relationship bullet: "the first element shall be a
    /// Relationship object (a URI or array of URIs)", which EXAMPLE 7 shows as
    /// the array of targets one observation named. The array is carried
    /// through as it stands.
    #[test]
    fn a_multi_target_relationship_collapses_into_the_array_of_uris_it_named() {
        let input = json!({
            "id": "urn:x",
            "type": "Building",
            "activeDevices": [{
                "type": "Relationship",
                "object": ["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562"],
                "observedAt": "2022-08-09T18:25:02Z"
            }]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(
            result["activeDevices"]["objects"],
            json!([[["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562"], "2022-08-09T18:25:02Z"]])
        );
    }

    /// Clause 4.5.9 keeps every other family on the member it names: `values`,
    /// `objects`, `languageMaps`, `jsons` and `vocabs`, the last three
    /// wrapping their value in a one-member object.
    #[test]
    fn the_other_families_keep_the_members_clause_4_5_9_names_for_them() {
        let input = json!({
            "id": "urn:x",
            "type": "Person",
            "speed": [{"type": "Property", "value": 25, "observedAt": "t1"}],
            "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "observedAt": "t1"}],
            "spouse": [{"type": "Relationship", "object": "urn:ngsi-ld:Person:1", "observedAt": "t1"}],
            "says": [{"type": "LanguageProperty", "languageMap": {"en": "yes"}, "observedAt": "t1"}],
            "gender": [{"type": "VocabProperty", "vocab": "Male", "observedAt": "t1"}],
            "tickets": [{"type": "JsonProperty", "json": {"id": "1"}, "observedAt": "t1"}]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["speed"]["values"], json!([[25, "t1"]]));
        pretty_assertions::assert_eq!(result["location"]["values"], json!([[{"type": "Point", "coordinates": [1.0, 2.0]}, "t1"]]));
        pretty_assertions::assert_eq!(result["spouse"]["objects"], json!([["urn:ngsi-ld:Person:1", "t1"]]));
        pretty_assertions::assert_eq!(result["says"]["languageMaps"], json!([[{"languageMap": {"en": "yes"}}, "t1"]]));
        pretty_assertions::assert_eq!(result["gender"]["vocabs"], json!([[{"vocab": "Male"}, "t1"]]));
        pretty_assertions::assert_eq!(result["tickets"]["jsons"], json!([[{"json": {"id": "1"}}, "t1"]]));
    }

    /// Clauses 4.5.0 and 4.5.21.2: a deleted `ListProperty` instance carries
    /// the array holding one single NGSI-LD Null, paired with the `deletedAt`
    /// that stands in for the Temporal Property it never observed.
    #[test]
    fn a_deleted_list_property_pairs_the_single_null_array_with_its_deletion_instant() {
        let input = json!({
            "id": "urn:x",
            "type": "Building",
            "period": [{"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"], "deletedAt": "2022-08-11T18:25:02Z"}]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["period"]["valueLists"], json!([[["urn:ngsi-ld:null"], "2022-08-11T18:25:02Z"]]));
        assert!(result["period"].get("value").is_none(), "{}", result["period"]);
    }

    /// Clause 4.5.9: the second element of each pair is the instant on the
    /// Temporal Property the query ran on, so a query over `modifiedAt` pairs
    /// each value with its `modifiedAt` rather than a null `observedAt`.
    #[test]
    fn each_pair_carries_the_instant_on_the_queried_temporal_property() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "currentStatus": [
                {"type": "Property", "value": "inTransitTo", "createdAt": "c1", "modifiedAt": "m1"},
                {"type": "Property", "value": "stoppedAt", "observedAt": "o2", "createdAt": "c2", "modifiedAt": "m2"}
            ]
        });
        for (timeproperty, expected) in [
            (TimeProp::Modified, json!([["inTransitTo", "m1"], ["stoppedAt", "m2"]])),
            (TimeProp::Created, json!([["inTransitTo", "c1"], ["stoppedAt", "c2"]])),
            (TimeProp::Observed, json!([["inTransitTo", null], ["stoppedAt", "o2"]])),
        ] {
            // clone: every axis projects its own copy of the one fixture.
            let result = TemporalValuesRenderer::new(input.clone(), timeproperty)
                .render()
                .expect("valid temporal values payload");
            pretty_assertions::assert_eq!(result["currentStatus"]["values"], expected, "{timeproperty}");
        }
    }

    /// A deleted instance on the `deletedAt` axis is paired with that instant.
    #[test]
    fn a_deletion_is_paired_with_its_deletion_instant_on_the_deleted_at_axis() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "d1"}]
        });
        let result = TemporalValuesRenderer::new(input, TimeProp::Deleted)
            .render()
            .expect("valid temporal values payload");
        pretty_assertions::assert_eq!(result["speed"]["values"], json!([["urn:ngsi-ld:null", "d1"]]));
    }

    #[test]
    fn temporal_values_rejects_a_malformed_dataset_uri() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 25, "observedAt": "t1", "datasetId": "not a URI"}]
        });
        let error = TemporalValuesRenderer::new(input, TimeProp::Observed)
            .render()
            .expect_err("malformed datasetId is rejected");
        assert!(matches!(error, TemporalValuesError::InvalidDatasetId));
    }

    #[test]
    fn temporal_values_rejects_an_unknown_attribute_type() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [{"type": "Unknown", "value": 25, "observedAt": "t1"}]
        });
        let error = TemporalValuesRenderer::new(input, TimeProp::Observed)
            .render()
            .expect_err("unknown attribute type is rejected");
        assert!(matches!(error, TemporalValuesError::InvalidAttributeType));
    }
}
