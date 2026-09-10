//! The NGSI-LD Null in expanded form: recognising the sentinel a patch deletes
//! a member with, and building the one a deletion is encoded by.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::{NULL_IRI, NULL_URI, expanded::properties};
use serde_json::{Value, json};

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

/// Returns `true` if the expanded attribute instance is a deletion sentinel.
///
/// Clauses 4.5.2.2 and 4.5.3.2 put the NGSI-LD Null on the right-hand side of
/// `value` and `object`, and clause 4.5.18 spells a `languageMap`'s as
/// `{"@none": "urn:ngsi-ld:null"}`, which expands to a bare `@value` node.
/// Clauses 4.5.21.2 and 4.5.22.2 give the two list families their own spelling:
/// `value` and `object` are prohibited on them, so the sentinel is the
/// "array containing one single NGSI-LD Null" of clause 4.5.0 written into
/// `valueList` or `objectList` — an expanded `@list` of exactly one null node.
#[must_use]
pub fn instance_is_null_sentinel(instance: &Value) -> bool {
    let Some(obj) = instance.as_object() else {
        return false;
    };

    let check_value_null = |key: &str, value_key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .and_then(|o| o.get(value_key))
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == NULL_URI.as_str() || s == NULL_IRI.as_str())
    };

    let check_list_null = |key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("@list"))
            .and_then(|v| v.as_array())
            .is_some_and(|members| members.len() == 1 && members.iter().all(member_is_null_sentinel))
    };

    check_value_null(properties::VALUE.as_str(), "@value")
        || check_value_null(properties::OBJECT.as_str(), "@id")
        || check_value_null(properties::LANGUAGE_MAP.as_str(), "@value")
        || check_list_null(properties::VALUE_LIST.as_str())
        || check_list_null(properties::OBJECT_LIST.as_str())
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
    /// carrying it is recognised by, for every family clauses 4.5.2.2,
    /// 4.5.3.2, 4.5.21.2 and 4.5.22.2 admit an NGSI-LD Null on.
    #[test]
    fn every_encoded_deletion_is_recognised_as_the_sentinel_it_encodes() {
        let families = [
            AttributeType::Property,
            AttributeType::GeoProperty,
            AttributeType::LanguageProperty,
            AttributeType::Relationship,
            AttributeType::ListProperty,
            AttributeType::ListRelationship,
        ];
        for attribute_type in families {
            let mut members = Map::new();
            members.insert("@type".to_owned(), json!([attribute_type.iri().as_str()]));
            members.insert(attribute_type.value_key_str().to_owned(), expanded_null_value(attribute_type));
            let instance = Value::Object(members);
            assert!(instance_is_null_sentinel(&instance), "{attribute_type}: {instance}");
        }
    }
}
