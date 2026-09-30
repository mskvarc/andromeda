//! Adapting a Merge Entity fragment to the attribute types the target entity
//! already fixes (Table 6.5.3.4-1 `format=simplified` and `lang`).
//!
//! A simplified fragment (clause 4.5.4) carries bare values: `"speed": 42`,
//! `"owner": "urn:ngsi-ld:Person:1"`. Which attribute family each value
//! belongs to is not in the fragment; Table 6.5.3.4-1 says the type of a
//! pre-existing attribute "shall remain unchanged" and an attempt to modify it
//! is a `BadRequest`. So before the clause 4.5.2.3 normalization would infer a
//! `Property` for every bare value, each value is wrapped as an instance of
//! the type the target entity holds for that attribute. An attribute the
//! target does not hold is left to normalization, which infers its type from
//! its shape.
//!
//! The same table's `lang` names the `languageMap` key a string (or string
//! array) merged into a pre-existing LanguageProperty updates, so such a value
//! becomes a one-entry language map and the clause 5.5.12 merge then replaces
//! only that key.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
use langtag::LangTagBuf;
use serde_json::{Map, Value};
use std::{collections::HashMap, mem::take, str::FromStr};
use thiserror::Error;

/// A fragment member that cannot be applied to the attribute the target holds.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MergeFragmentError {
    /// A simplified fragment declared a type other than the target's
    /// (Table 6.5.3.4-1: "any attempt to modify the type of an Attribute shall
    /// result in a `BadRequest` error").
    #[error("attribute {attribute} is a {existing} on the target entity; a simplified merge cannot change it to a {declared}")]
    TypeChange {
        /// The fragment member.
        attribute: AttributeName,
        /// The type the target entity holds.
        existing: AttributeType,
        /// The type the fragment declared.
        declared: AttributeType,
    },
    /// A string was merged into a `LanguageProperty` without the `lang` that
    /// names its `languageMap` key.
    #[error("attribute {attribute} is a LanguageProperty; merging a string into it requires the lang parameter")]
    LanguageKeyRequired {
        /// The fragment member.
        attribute: AttributeName,
    },
}

/// The attribute family each top-level member of an expanded entity holds,
/// keyed by expanded attribute IRI. An instance declaring no recognised
/// `@type` is a Property (clause 4.5.2).
#[must_use]
pub fn existing_attribute_types(expanded_entity: &Map<String, Value>) -> HashMap<String, AttributeType> {
    expanded_entity
        .iter()
        .filter(|(key, _)| !key.starts_with('@'))
        .filter_map(|(key, member)| {
            let first_instance = member.as_array().and_then(|instances| instances.first()).or(Some(member))?;
            let declared = first_instance
                .get("@type")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find_map(AttributeType::from_expanded_iri)
                .unwrap_or(AttributeType::Property);
            Some((key.clone(), declared)) // clone: the map owns the IRIs it is keyed by
        })
        .collect()
}

/// Rewrite one fragment member for the type the target entity holds under
/// `attribute`, or leave it alone when the target holds no such attribute.
///
/// # Errors
///
/// [`MergeFragmentError::TypeChange`] when a simplified fragment declares a
/// different type; [`MergeFragmentError::LanguageKeyRequired`] when a string
/// meets a `LanguageProperty` with no `lang`.
pub fn adapt_member(
    attribute: &AttributeName,
    member: &mut Value,
    existing: Option<AttributeType>,
    representation: MergeRepresentation,
    lang: Option<&LangTagBuf>,
) -> Result<(), MergeFragmentError> {
    if member.as_str() == Some("urn:ngsi-ld:null") {
        return Ok(());
    }
    let Some(existing) = existing else {
        return Ok(());
    };
    match representation {
        MergeRepresentation::Normalized => adapt_language_member(attribute, member, existing, lang),
        MergeRepresentation::Simplified => adapt_simplified_member(attribute, member, existing, lang),
    }
}

/// A normalized `LanguageProperty` node whose `languageMap` is a bare string or
/// string array takes `lang` as that string's key.
fn adapt_language_member(attribute: &AttributeName, member: &mut Value, existing: AttributeType, lang: Option<&LangTagBuf>) -> Result<(), MergeFragmentError> {
    if existing != AttributeType::LanguageProperty {
        return Ok(());
    }
    let Some(node) = member.as_object_mut() else {
        return Ok(());
    };
    let language_map_key = AttributeType::LanguageProperty.compact_value_key();
    if node.get(language_map_key).is_some_and(is_string_or_strings) {
        let Some(tag) = lang else {
            return Err(MergeFragmentError::LanguageKeyRequired {
                attribute: attribute.clone(), // clone: the error owns the fragment member it names.
            });
        };
        if let Some(strings) = node.remove(language_map_key) {
            node.insert(language_map_key.to_string(), language_map_entry(tag, strings));
        }
    }
    Ok(())
}

/// A simplified member becomes an instance of the target's type; one that
/// already declares a type must declare the target's.
fn adapt_simplified_member(
    attribute: &AttributeName,
    member: &mut Value,
    existing: AttributeType,
    lang: Option<&LangTagBuf>,
) -> Result<(), MergeFragmentError> {
    if let Some(declared) = member.as_object().and_then(declared_attribute_type) {
        if declared != existing {
            return Err(MergeFragmentError::TypeChange {
                attribute: attribute.clone(), // clone: the error owns the fragment member it names.
                existing,
                declared,
            });
        }
        return adapt_language_member(attribute, member, existing, lang);
    }
    let bare = take(member);
    let value = match existing {
        AttributeType::LanguageProperty if is_string_or_strings(&bare) => {
            let Some(tag) = lang else {
                return Err(MergeFragmentError::LanguageKeyRequired {
                    attribute: attribute.clone(), // clone: the error owns the fragment member it names.
                });
            };
            language_map_entry(tag, bare)
        }
        AttributeType::Property
        | AttributeType::Relationship
        | AttributeType::GeoProperty
        | AttributeType::VocabProperty
        | AttributeType::LanguageProperty
        | AttributeType::JsonProperty
        | AttributeType::ListProperty
        | AttributeType::ListRelationship => bare,
    };
    let mut instance = Map::with_capacity(2);
    instance.insert("type".to_string(), Value::String(existing.to_string()));
    instance.insert(existing.compact_value_key().to_string(), value);
    *member = Value::Object(instance);
    Ok(())
}

/// The clause 5.2.4 type a node declares, when its `type` names one.
fn declared_attribute_type(node: &Map<String, Value>) -> Option<AttributeType> {
    AttributeType::from_str(node.get("type")?.as_str()?).ok()
}

/// Whether a value is a string or an array of strings: the shapes Table
/// 6.5.3.4-1 `lang` gives a key to.
fn is_string_or_strings(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Array(items) => !items.is_empty() && items.iter().all(Value::is_string),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
    }
}

/// A language map holding `strings` under `tag` alone.
fn language_map_entry(tag: &LangTagBuf, strings: Value) -> Value {
    let mut map = Map::with_capacity(1);
    map.insert(tag.to_string(), strings);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use crate::merge_fragment::{MergeFragmentError, adapt_member, existing_attribute_types};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
    use langtag::LangTagBuf;
    use serde_json::{Value, json};

    fn attr() -> AttributeName {
        "attr".parse().expect("a valid attribute name")
    }

    fn tag(raw: &str) -> LangTagBuf {
        LangTagBuf::new(raw.to_string()).expect("valid tag")
    }

    fn adapt(
        member: Value,
        existing: Option<AttributeType>,
        representation: MergeRepresentation,
        lang: Option<&LangTagBuf>,
    ) -> Result<Value, MergeFragmentError> {
        let mut member = member;
        adapt_member(&attr(), &mut member, existing, representation, lang)?;
        Ok(member)
    }

    #[test]
    fn the_existing_types_are_read_off_the_expanded_payload() {
        let expanded = json!({
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}],
            "https://example.org/owner": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:x"}]}],
            "https://example.org/untyped": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
        });
        let types = existing_attribute_types(expanded.as_object().expect("object"));
        pretty_assertions::assert_eq!(types.get("https://example.org/speed"), Some(&AttributeType::Property));
        pretty_assertions::assert_eq!(types.get("https://example.org/owner"), Some(&AttributeType::Relationship));
        pretty_assertions::assert_eq!(types.get("https://example.org/untyped"), Some(&AttributeType::Property));
        assert!(!types.contains_key("@id"));
    }

    #[test]
    fn a_simplified_bare_value_takes_the_targets_type_and_value_member() {
        // Table 6.5.3.4-1 `format=simplified`: the type of the pre-existing
        // attribute is preserved, so a bare string merged into a Relationship
        // is its object, not a Property value.
        let adapted = adapt(
            json!("urn:ngsi-ld:Person:2"),
            Some(AttributeType::Relationship),
            MergeRepresentation::Simplified,
            None,
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(adapted, json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:2"}));
        let json_value = adapt(json!({"k": 1}), Some(AttributeType::JsonProperty), MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(json_value, json!({"type": "JsonProperty", "json": {"k": 1}}));
        let list = adapt(json!([1, 2]), Some(AttributeType::ListProperty), MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(list, json!({"type": "ListProperty", "valueList": [1, 2]}));
    }

    #[test]
    fn a_simplified_value_for_an_attribute_the_target_lacks_is_left_to_inference() {
        let untouched = adapt(json!(42), None, MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(untouched, json!(42));
    }

    #[test]
    fn a_simplified_fragment_declaring_another_type_is_a_type_change() {
        let error = adapt(
            json!({"type": "Property", "value": "urn:ngsi-ld:Person:2"}),
            Some(AttributeType::Relationship),
            MergeRepresentation::Simplified,
            None,
        )
        .expect_err("a type change is rejected");
        pretty_assertions::assert_eq!(
            error,
            MergeFragmentError::TypeChange {
                attribute: attr(),
                existing: AttributeType::Relationship,
                declared: AttributeType::Property,
            }
        );
        let same = adapt(
            json!({"type": "Property", "value": 1}),
            Some(AttributeType::Property),
            MergeRepresentation::Simplified,
            None,
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(same, json!({"type": "Property", "value": 1}));
    }

    #[test]
    fn a_string_merged_into_a_language_property_becomes_the_lang_key() {
        let de = tag("de");
        let simplified = adapt(json!("Tuer"), Some(AttributeType::LanguageProperty), MergeRepresentation::Simplified, Some(&de)).expect("adapts");
        pretty_assertions::assert_eq!(simplified, json!({"type": "LanguageProperty", "languageMap": {"de": "Tuer"}}));
        let normalized = adapt(
            json!({"type": "LanguageProperty", "languageMap": ["Tuer", "Tor"]}),
            Some(AttributeType::LanguageProperty),
            MergeRepresentation::Normalized,
            Some(&de),
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(normalized, json!({"type": "LanguageProperty", "languageMap": {"de": ["Tuer", "Tor"]}}));
        let missing = adapt(json!("Tuer"), Some(AttributeType::LanguageProperty), MergeRepresentation::Simplified, None).expect_err("lang is required");
        pretty_assertions::assert_eq!(missing, MergeFragmentError::LanguageKeyRequired { attribute: attr() });
    }

    #[test]
    fn a_normalized_fragment_and_a_null_sentinel_pass_through_untouched() {
        let node = json!({"type": "Property", "value": 5});
        pretty_assertions::assert_eq!(
            adapt(node.clone(), Some(AttributeType::Relationship), MergeRepresentation::Normalized, None).expect("adapts"),
            node
        );
        pretty_assertions::assert_eq!(
            adapt(
                json!("urn:ngsi-ld:null"),
                Some(AttributeType::Relationship),
                MergeRepresentation::Simplified,
                None
            )
            .expect("adapts"),
            json!("urn:ngsi-ld:null")
        );
    }
}
