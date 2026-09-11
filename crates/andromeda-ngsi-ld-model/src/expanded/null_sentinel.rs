//! The NGSI-LD Null in expanded form: recognising the sentinel a patch deletes
//! a member with, building the one a deletion is encoded by, and stripping the
//! instructions back out of a payload that is about to be recorded as an
//! observation.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::{NULL_IRI, NULL_URI, expanded::properties};
use serde_json::{Map, Value, json};

/// Returns `true` when an expanded first-level member is the bare NGSI-LD Null.
///
/// This is the expansion of `"attribute": "urn:ngsi-ld:null"` in a merge
/// fragment. Clause 5.5.12 makes it remove the complete target member rather
/// than one dataset-partitioned attribute instance.
pub fn member_is_null_sentinel(instance: &Value) -> bool {
    instance
        .as_object()
        .and_then(|object| object.get("@value").or_else(|| object.get("@id")))
        .and_then(Value::as_str)
        .is_some_and(|value| value == NULL_URI.as_str() || value == NULL_IRI.as_str())
}

/// Whether one member of an expanded `@list` is the NGSI-LD Null.
///
/// Clause 4.5.22.2 makes each element of a normalized `objectList` "a JSON
/// object containing a single Attribute with a key called `object`", while
/// clause 4.5.22.3 lets a Context Producer write the concise spelling, where
/// "each string in the array holds a valid URI". The single NGSI-LD Null of
/// clause 4.5.0 therefore reaches the list either as a bare null node or
/// wrapped in that object, and both spell the same deletion.
fn list_member_is_null_sentinel(member: &Value) -> bool {
    member_is_null_sentinel(member)
        || member
            .get(properties::OBJECT.as_str())
            .and_then(Value::as_array)
            .and_then(|nodes| nodes.first())
            .is_some_and(member_is_null_sentinel)
}

/// Returns `true` if the expanded attribute instance is a deletion sentinel.
///
/// Clauses 4.5.2.2 and 4.5.3.2 put the NGSI-LD Null on the right-hand side of
/// `value` and `object`, and clause 4.5.18 spells a `languageMap`'s as
/// `{"@none": "urn:ngsi-ld:null"}`, which expands to a bare `@value` node.
/// Clauses 4.5.21.2 and 4.5.22.2 give the two list families their own spelling:
/// `value` and `object` are prohibited on them, so the sentinel is the
/// "array containing one single NGSI-LD Null" of clause 4.5.0 written into
/// `valueList` or `objectList` — an expanded `@list` of exactly one null node.
///
/// `VocabProperty` and `JsonProperty` are reached through the generalization
/// their own clauses state rather than through a sentence of their own. Clause
/// 4.5.20.2 prohibits `value` on a `VocabProperty` "as value is a
/// generalization of vocab", and clause 4.5.24.2 says the same of `json`; the
/// deletion meaning clause 4.5.2.2 gives the generalized member therefore
/// belongs to the specialized one. Keeping the recognition total over all eight
/// families is also what makes it the inverse of [`expanded_null_value`], which
/// emits a deletion for every one of them: what the broker renders for an ended
/// instance is what it reads back as a deletion.
#[must_use]
pub fn instance_is_null_sentinel(instance: &Value) -> bool {
    let Some(members) = instance.as_object() else {
        return false;
    };

    AttributeType::ALL.iter().any(|family| {
        let Some(member) = members.get(family.value_key_str()) else {
            return false;
        };
        match family {
            // The two ordered families carry the sentinel inside the `@list`
            // expansion of their value member, never beside it.
            AttributeType::ListProperty | AttributeType::ListRelationship => member
                .as_array()
                .and_then(|nodes| nodes.first())
                .and_then(|node| node.get("@list"))
                .and_then(Value::as_array)
                .is_some_and(|nodes| nodes.len() == 1 && nodes.iter().all(list_member_is_null_sentinel)),
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::Relationship
            | AttributeType::LanguageProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty => member.as_array().and_then(|nodes| nodes.first()).is_some_and(member_is_null_sentinel),
        }
    })
}

/// The expanded member value that encodes a deleted instance of one attribute
/// family (clause 4.5.0).
///
/// Clause 4.5.0 writes a deletion as the URI `urn:ngsi-ld:null` wherever a URI
/// string is a valid JSON value, and requires "an array with one single NGSI-LD
/// Null" where an array is needed — which is the case for the two list
/// families, whose value member is an ordered array and whose `value` /
/// `object` generalization clauses 4.5.21.2 and 4.5.22.2 prohibit outright.
///
/// Each family gets the node that compacts back to the spelling its own clause
/// mandates: an `@id` node under the terms the core context coerces to `@id`
/// and `@vocab`, the JSON literal under `json`, and a plain `@value` node
/// elsewhere — including inside a list, so `valueList` and `objectList`
/// compact to a one-element array of the null URI rather than to an array of
/// node objects.
///
/// The member this belongs under is [`AttributeType::value_key`]; the two are
/// always written together.
#[must_use]
pub fn expanded_null_value(attribute_type: AttributeType) -> Value {
    match attribute_type {
        AttributeType::Relationship | AttributeType::VocabProperty => json!([{"@id": NULL_URI.as_str()}]),
        AttributeType::JsonProperty => json!([{"@value": NULL_URI.as_str(), "@type": "@json"}]),
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::LanguageProperty => json!([{"@value": NULL_URI.as_str()}]),
        AttributeType::ListProperty | AttributeType::ListRelationship => json!([{"@list": [{"@value": NULL_URI.as_str()}]}]),
    }
}

/// The same payload with every NGSI-LD Null deletion instruction removed, or
/// `None` when it carries none.
///
/// Clause 4.5.0 introduces the sentinel "to nevertheless allow deletions as
/// part of NGSI-LD operations that update NGSI-LD data": it is an instruction,
/// not a value. Clause 4.5.6 makes the Temporal Evolution of an Entity a record
/// of what was observed, and what a deletion left behind is recorded instead by
/// the tombstone clauses 4.5.7 and 4.5.8 prescribe. An observation built
/// straight from a patch Fragment would therefore record `urn:ngsi-ld:null` as
/// the value of the very member the Fragment asked to remove -- and for a
/// closed-vocabulary member such as `unitCode` it is not even a well-formed
/// one, so the write would be refused rather than merely misrecorded.
///
/// A member is dropped when its value is the NGSI-LD Null in either spelling
/// clause 4.5.0 gives it: the bare URI a non-reified member takes, and the
/// value-position sentinel of a reified attribute. `datasetId` is never
/// dropped, because clauses 5.5.8 and 5.5.12 both state it "cannot be deleted
/// by setting it to the value urn:ngsi-ld:null", so a sentinel standing there
/// is refused on admission rather than obeyed here.
#[must_use]
pub fn without_null_sentinels(payload: &Value) -> Option<Value> {
    match payload {
        Value::Array(nodes) => {
            let stripped: Vec<Option<Value>> = nodes.iter().map(without_null_sentinels).collect();
            stripped.iter().any(Option::is_some).then(|| {
                Value::Array(
                    nodes
                        .iter()
                        .zip(stripped)
                        // clone: an unchanged node is borrowed and the rebuilt array owns its elements.
                        .map(|(node, replacement)| replacement.unwrap_or_else(|| node.clone()))
                        .collect(),
                )
            })
        }
        Value::Object(members) => strip_node_members(members),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// Strip the deletion instructions out of one expanded node's members.
fn strip_node_members(members: &Map<String, Value>) -> Option<Value> {
    let mut stripped = Map::with_capacity(members.len());
    let mut changed = false;
    for (member, value) in members {
        if member.starts_with('@') || member == properties::DATASET_ID.as_str() {
            // clone: the keyword or identity member is borrowed and the rebuilt node owns it.
            stripped.insert(member.clone(), value.clone());
            continue;
        }
        if member_value_is_null_sentinel(value) {
            changed = true;
            continue;
        }
        match without_null_sentinels(value) {
            Some(replacement) => {
                changed = true;
                stripped.insert(member.clone(), replacement); // clone: the rebuilt node owns its member name.
            }
            // clone: an unchanged member is borrowed and the rebuilt node owns it.
            None => {
                stripped.insert(member.clone(), value.clone());
            }
        }
    }
    changed.then_some(Value::Object(stripped))
}

/// Whether an expanded member's value is the NGSI-LD Null of clause 4.5.0.
///
/// Expansion array-wraps every member, and clause 4.5.0 admits exactly one
/// NGSI-LD Null where an array is required, so a member holding more than one
/// node is data even when one of them is the sentinel.
fn member_value_is_null_sentinel(member: &Value) -> bool {
    let Some(nodes) = member.as_array() else {
        return member_is_null_sentinel(member) || instance_is_null_sentinel(member);
    };
    nodes.len() == 1 && nodes.iter().all(|node| member_is_null_sentinel(node) || instance_is_null_sentinel(node))
}

#[cfg(test)]
mod tests {
    use crate::expanded::null_sentinel::{expanded_null_value, instance_is_null_sentinel, member_is_null_sentinel};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::{Map, Value, json};

    #[test]
    fn null_sentinel_has_value() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn null_sentinel_has_object() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn non_null_is_not_sentinel() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
        });
        assert!(!instance_is_null_sentinel(&instance));
    }

    #[test]
    fn expanded_first_level_null_is_a_member_sentinel() {
        assert!(member_is_null_sentinel(&json!({"@value": "urn:ngsi-ld:null"})));
        assert!(!member_is_null_sentinel(&json!({"@value": "ordinary"})));
    }

    /// Clause 4.5.21.2: a `ListProperty` is deleted by a `valueList` holding
    /// the single NGSI-LD Null of clause 4.5.0, never by a `value`, which that
    /// clause prohibits on the family.
    #[test]
    fn a_value_list_of_one_null_is_a_list_property_deletion() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    /// Clause 4.5.22.2: the same single-null array deletes a
    /// `ListRelationship` through `objectList`, whichever keyword the writer's
    /// `@context` coerced the member to.
    #[test]
    fn an_object_list_of_one_null_is_a_list_relationship_deletion() {
        for node in [json!({"@value": "urn:ngsi-ld:null"}), json!({"@id": "urn:ngsi-ld:null"})] {
            let instance = json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [node]}]
            });
            assert!(instance_is_null_sentinel(&instance), "{instance}");
        }
    }

    /// Clause 4.5.22.2 spells each element of a normalized `objectList` as "a
    /// JSON object containing a single Attribute with a key called `object`",
    /// so a `ListRelationship` deletion written that way carries the null one
    /// level deeper than the concise spelling does.
    #[test]
    fn a_normalized_object_list_of_one_null_is_a_list_relationship_deletion() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
            "https://uri.etsi.org/ngsi-ld/hasObjectList": [{
                "@list": [{"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]}]
            }]
        });
        assert!(instance_is_null_sentinel(&instance), "{instance}");
    }

    /// Clause 4.5.0 admits exactly one NGSI-LD Null in the array, so a list
    /// that merely contains one alongside real members is data, not a
    /// deletion, and an ordinary list is neither.
    #[test]
    fn a_list_that_is_not_a_single_null_is_not_a_deletion() {
        let mixed = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "urn:ngsi-ld:null"}, {"@value": 2}]}]
        });
        let ordinary = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": 1}]}]
        });
        assert!(!instance_is_null_sentinel(&mixed));
        assert!(!instance_is_null_sentinel(&ordinary));
    }

    /// Clause 4.5.0: a deleted list family is encoded as an array holding one
    /// single NGSI-LD Null, and every other family as the bare null in the
    /// keyword its own value member takes.
    #[test]
    fn every_family_encodes_its_deletion_the_way_clause_4_5_0_spells_it() {
        let expected = [
            (AttributeType::Property, json!([{"@value": "urn:ngsi-ld:null"}])),
            (AttributeType::GeoProperty, json!([{"@value": "urn:ngsi-ld:null"}])),
            (AttributeType::LanguageProperty, json!([{"@value": "urn:ngsi-ld:null"}])),
            (AttributeType::Relationship, json!([{"@id": "urn:ngsi-ld:null"}])),
            (AttributeType::VocabProperty, json!([{"@id": "urn:ngsi-ld:null"}])),
            (AttributeType::JsonProperty, json!([{"@value": "urn:ngsi-ld:null", "@type": "@json"}])),
            (AttributeType::ListProperty, json!([{"@list": [{"@value": "urn:ngsi-ld:null"}]}])),
            (AttributeType::ListRelationship, json!([{"@list": [{"@value": "urn:ngsi-ld:null"}]}])),
        ];
        for (attribute_type, encoded) in expected {
            pretty_assertions::assert_eq!(expanded_null_value(attribute_type), encoded, "{attribute_type}");
        }
    }

    /// The two halves agree: what a deletion is encoded as is what a patch
    /// carrying it is recognised by, for every one of the eight families --
    /// the six clauses 4.5.2.2, 4.5.3.2, 4.5.18, 4.5.21.2 and 4.5.22.2 name
    /// outright, and the two clauses 4.5.20.2 and 4.5.24.2 reach by declaring
    /// `value` a generalization of `vocab` and of `json`.
    #[test]
    fn every_encoded_deletion_is_recognised_as_the_sentinel_it_encodes() {
        for attribute_type in AttributeType::ALL {
            let mut members = Map::new();
            members.insert("@type".to_owned(), json!([attribute_type.iri().as_str()]));
            members.insert(attribute_type.value_key_str().to_owned(), expanded_null_value(attribute_type));
            let instance = Value::Object(members);
            assert!(instance_is_null_sentinel(&instance), "{attribute_type}: {instance}");
        }
    }
}
