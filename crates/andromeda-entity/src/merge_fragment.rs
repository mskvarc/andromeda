//! Adapting a Merge Entity fragment to the attribute types the target entity
//! already fixes (Table 6.5.3.4-1 `format=simplified` and `lang`).
//!
//! A simplified fragment (clause 4.5.4) carries bare values: `"speed": 42`,
//! `"owner": "urn:ngsi-ld:Person:1"`. Which attribute family each value
//! belongs to is not in the fragment; Table 6.5.3.4-1 says the type of a
//! pre-existing attribute "shall remain unchanged" and an attempt to modify it
//! is a `BadRequest`. So every member that names no family is read as the
//! value of the type the target entity holds for that attribute, from the JSON
//! it was written as. An attribute the target does not hold keeps the family
//! the clause 4.5.2.3 reading inferred from its shape.
//!
//! The same table's `lang` names the `languageMap` key a string (or string
//! array) merged into a pre-existing `LanguageProperty` updates, so such a
//! value becomes a one-entry language map and the clause 5.5.12 merge then
//! replaces only that key.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_model::{
    attribute::{
        language_property::{LanguageMap, LanguageMapKey},
        merge_instance::{Declaration, MergeInstance},
        merge_member::MergeMember,
        merge_value::{LanguageMapPatch, MergeValue},
        patched::Patched,
    },
    node_refusal::RefusedNode,
};
use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
use indexmap::IndexMap;
use langtag::LangTagBuf;
use std::mem::replace;
use thiserror::Error;

/// A fragment member that cannot be applied to the attribute the target holds.
#[derive(Debug, Error)]
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
    /// A simplified fragment wrote a value the type the target holds does not
    /// hold.
    #[error("attribute {attribute} is a {existing} on the target entity, which does not hold the value written: {refusal}")]
    ValueOfAnotherType {
        /// The fragment member.
        attribute: AttributeName,
        /// The type the target entity holds.
        existing: AttributeType,
        /// Why the value is not one of `existing`.
        #[source]
        refusal: RefusedNode,
    },
}

/// The attribute family the target entity holds under the expanded name
/// `attribute`: the family of its first instance, every instance of one
/// attribute sharing it (clause 4.5.5).
#[must_use]
pub fn held_attribute_family(target: &EntityState, attribute: &str) -> Option<AttributeType> {
    target.member_instance_families(attribute).next()
}

/// Rewrite one fragment member for the type the target entity holds under
/// `attribute`, or leave it alone when the target holds no such attribute.
///
/// # Errors
///
/// [`MergeFragmentError::TypeChange`] when a simplified fragment declares a
/// different type; [`MergeFragmentError::ValueOfAnotherType`] when a
/// simplified fragment writes a value the held type does not hold;
/// [`MergeFragmentError::LanguageKeyRequired`] when a string meets a
/// `LanguageProperty` with no `lang`.
pub fn adapt_member(
    attribute: &AttributeName,
    member: &mut MergeMember,
    existing: Option<AttributeType>,
    representation: MergeRepresentation,
    lang: Option<&LangTagBuf>,
) -> Result<(), MergeFragmentError> {
    let Some(existing) = existing else {
        return Ok(());
    };
    member.instances_mut().try_for_each(|instance| {
        if representation == MergeRepresentation::Simplified {
            adapt_simplified_instance(attribute, instance, existing)?;
        }
        adapt_language_instance(attribute, instance, existing, lang)
    })
}

/// A simplified instance that names no family becomes an instance of the
/// target's type, read from what it was written as; one that names a family
/// must name the target's.
fn adapt_simplified_instance(attribute: &AttributeName, instance: &mut MergeInstance, existing: AttributeType) -> Result<(), MergeFragmentError> {
    match &instance.declaration {
        Declaration::Typed => match instance.family {
            Some(declared) if declared != existing => Err(MergeFragmentError::TypeChange {
                attribute: attribute.clone(), // clone: the error owns the fragment member it names.
                existing,
                declared,
            }),
            Some(_) | None => Ok(()),
        },
        Declaration::Concise(written) => {
            *instance = MergeInstance::of_family_value(existing, written).map_err(|refusal| MergeFragmentError::ValueOfAnotherType {
                attribute: attribute.clone(), // clone: the error owns the fragment member it names.
                existing,
                refusal,
            })?;
            Ok(())
        }
    }
}

/// A `languageMap` written as untagged strings, merged into a
/// `LanguageProperty`, takes `lang` as their key.
fn adapt_language_instance(
    attribute: &AttributeName,
    instance: &mut MergeInstance,
    existing: AttributeType,
    lang: Option<&LangTagBuf>,
) -> Result<(), MergeFragmentError> {
    if existing != AttributeType::LanguageProperty {
        return Ok(());
    }
    let Some(Patched::Set(MergeValue::LanguageProperty(language_map))) = &mut instance.value else {
        return Ok(());
    };
    let LanguageMapPatch::Untagged(_) = language_map else {
        return Ok(());
    };
    let Some(tag) = lang else {
        return Err(MergeFragmentError::LanguageKeyRequired {
            attribute: attribute.clone(), // clone: the error owns the fragment member it names.
        });
    };
    let LanguageMapPatch::Untagged(strings) = replace(language_map, LanguageMapPatch::Map(LanguageMap::default())) else {
        return Ok(());
    };
    // clone: the language map owns its key, the request keeps its `lang`.
    *language_map = LanguageMapPatch::Map(LanguageMap(IndexMap::from([(LanguageMapKey::Tag(tag.clone()), strings)])));
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::merge_fragment::{MergeFragmentError, adapt_member, held_attribute_family};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_model::attribute::{member_reading::MemberReading, merge_member::MergeMember};
    use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
    use langtag::LangTagBuf;
    use serde_json::{Value, json};

    fn attr() -> AttributeName {
        "attr".parse().expect("a valid attribute name")
    }

    fn tag(raw: &str) -> LangTagBuf {
        LangTagBuf::new(raw.to_string()).expect("valid tag")
    }

    /// The member `written` adapts to, as the JSON it writes.
    fn adapt(
        written: &Value,
        existing: Option<AttributeType>,
        representation: MergeRepresentation,
        lang: Option<&LangTagBuf>,
    ) -> Result<Value, MergeFragmentError> {
        let mut member = MergeMember::from_node(attr().as_str(), written).expect("a merge member");
        adapt_member(&attr(), &mut member, existing, representation, lang)?;
        Ok(serde_json::to_value(&member).expect("the member is written"))
    }

    #[test]
    fn the_held_types_are_read_off_the_stored_state() {
        let expanded = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}],
            "https://example.org/owner": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:x"}]}],
            "https://example.org/untyped": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
        }]);
        let target = EntityState::from_slice(&serde_json::to_vec(&expanded).expect("JSON")).expect("a stored entity");
        pretty_assertions::assert_eq!(held_attribute_family(&target, "https://example.org/speed"), Some(AttributeType::Property));
        pretty_assertions::assert_eq!(held_attribute_family(&target, "https://example.org/owner"), Some(AttributeType::Relationship));
        pretty_assertions::assert_eq!(held_attribute_family(&target, "https://example.org/untyped"), Some(AttributeType::Property));
        pretty_assertions::assert_eq!(held_attribute_family(&target, "https://example.org/absent"), None);
    }

    /// Table 6.5.3.4-1 `format=simplified`: the type of the pre-existing
    /// attribute is preserved, so a bare string merged into a Relationship is
    /// its object, not a Property value.
    #[test]
    fn a_simplified_bare_value_takes_the_targets_type_and_value_member() {
        let adapted = adapt(
            &json!("urn:ngsi-ld:Person:2"),
            Some(AttributeType::Relationship),
            MergeRepresentation::Simplified,
            None,
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(adapted, json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:2"}));
        let json_value = adapt(&json!({"k": 1}), Some(AttributeType::JsonProperty), MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(json_value, json!({"type": "JsonProperty", "json": {"k": 1}}));
        let list = adapt(&json!([1, 2]), Some(AttributeType::ListProperty), MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(list, json!({"type": "ListProperty", "valueList": [1, 2]}));
    }

    /// A simplified object with a `value` member names no family: it is the
    /// value itself, read as the held type holds it.
    #[test]
    fn a_simplified_object_is_the_value_of_the_held_type() {
        let adapted = adapt(
            &json!({"value": 5, "unit": "x"}),
            Some(AttributeType::JsonProperty),
            MergeRepresentation::Simplified,
            None,
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(adapted, json!({"type": "JsonProperty", "json": {"value": 5, "unit": "x"}}));
    }

    #[test]
    fn a_simplified_value_for_an_attribute_the_target_lacks_is_left_to_inference() {
        let untouched = adapt(&json!(42), None, MergeRepresentation::Simplified, None).expect("adapts");
        pretty_assertions::assert_eq!(untouched, json!({"type": "Property", "value": 42}));
    }

    #[test]
    fn a_simplified_fragment_declaring_another_type_is_a_type_change() {
        let error = adapt(
            &json!({"type": "Property", "value": "urn:ngsi-ld:Person:2"}),
            Some(AttributeType::Relationship),
            MergeRepresentation::Simplified,
            None,
        )
        .expect_err("a type change is rejected");
        assert!(
            matches!(
                error,
                MergeFragmentError::TypeChange {
                    existing: AttributeType::Relationship,
                    declared: AttributeType::Property,
                    ..
                }
            ),
            "{error:?}"
        );
        let same = adapt(
            &json!({"type": "Property", "value": 1}),
            Some(AttributeType::Property),
            MergeRepresentation::Simplified,
            None,
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(same, json!({"type": "Property", "value": 1}));
    }

    /// A simplified value the held type cannot hold is refused naming the
    /// attribute.
    #[test]
    fn a_simplified_value_the_held_type_does_not_hold_is_refused() {
        let error = adapt(&json!(42), Some(AttributeType::Relationship), MergeRepresentation::Simplified, None).expect_err("a number is no object");
        assert!(
            matches!(
                error,
                MergeFragmentError::ValueOfAnotherType {
                    existing: AttributeType::Relationship,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_string_merged_into_a_language_property_becomes_the_lang_key() {
        let de = tag("de");
        let simplified = adapt(
            &json!("Tuer"),
            Some(AttributeType::LanguageProperty),
            MergeRepresentation::Simplified,
            Some(&de),
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(simplified, json!({"type": "LanguageProperty", "languageMap": {"de": "Tuer"}}));
        let normalized = adapt(
            &json!({"type": "LanguageProperty", "languageMap": ["Tuer", "Tor"]}),
            Some(AttributeType::LanguageProperty),
            MergeRepresentation::Normalized,
            Some(&de),
        )
        .expect("adapts");
        pretty_assertions::assert_eq!(normalized, json!({"type": "LanguageProperty", "languageMap": {"de": ["Tuer", "Tor"]}}));
        let missing = adapt(&json!("Tuer"), Some(AttributeType::LanguageProperty), MergeRepresentation::Simplified, None).expect_err("lang is required");
        assert!(matches!(missing, MergeFragmentError::LanguageKeyRequired { .. }), "{missing:?}");
    }

    #[test]
    fn a_normalized_fragment_and_a_null_sentinel_pass_through_untouched() {
        let node = json!({"type": "Property", "value": 5});
        pretty_assertions::assert_eq!(
            adapt(&node, Some(AttributeType::Relationship), MergeRepresentation::Normalized, None).expect("adapts"),
            node
        );
        pretty_assertions::assert_eq!(
            adapt(
                &json!("urn:ngsi-ld:null"),
                Some(AttributeType::Relationship),
                MergeRepresentation::Simplified,
                None
            )
            .expect("adapts"),
            json!("urn:ngsi-ld:null")
        );
    }
}
