//! Refusing a `@context` written below the top level of an NGSI-LD document.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "be embedded into NGSI-LD Attributes, i.e. there cannot be parts of the
//! user @context other than at the top level of the NGSI-LD document", and
//! that "parts of user @context that are not following the two points above
//! should result in an error of type `BadRequestData`, because JSON-LD Scoped
//! Contexts and nested embedded @context could be used to modify terms defined
//! in the Core @context or to reshape NGSI-LD Elements during the expansion of
//! terms".
//!
//! The rule is decidable on the compact payload a client wrote and on nothing
//! else, because the transport layer lifts the document's own top-level
//! `@context` out of the payload before any handler sees it: every `@context`
//! still standing in what reaches this module is by definition one clause 5.5.7
//! does not admit. The walk is therefore unconditional -- every object below a
//! top-level member is examined, including a Property's `value`, a
//! `GeoProperty`'s geometry and a `LanguageProperty`'s `languageMap`, all of
//! which the JSON-LD processor traverses and would apply an embedded context
//! from.
//!
//! The one exemption is a native JSON literal. Clause 5.5.7 closes with "note
//! that the `JsonProperty` is designed to hold native JSON values which are by
//! definition not available for expansion and compaction via an @context.
//! Therefore, the given short-hand name is always used for accessing JSON keys
//! within a `JsonProperty` json element": the processor never expands inside
//! one, so a key spelled `@context` there is data rather than a context.
//!
//! This is a statement about a representation, which is why it is stated here
//! rather than beside one write path. Which of the write paths run it is
//! decided by those paths: a payload a Context Source or another broker sent
//! back is not a user `@context` and is never screened.

use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use thiserror::Error;

/// The member a JSON-LD document declares its context in.
const CONTEXT_MEMBER: &str = "@context";

/// The compacted members the NGSI-LD Core `@context` coerces to
/// `@type: "@json"`, whose content is a native JSON literal rather than
/// JSON-LD.
///
/// Clause 5.5.7 gives the Core `@context` absolute precedence and states that
/// it "is protected and cannot be overridden", so a user `@context` cannot
/// redefine one of these into a member the processor would traverse. What
/// stands under one of them is therefore data in every request, whatever
/// contexts it carries.
const NATIVE_JSON_MEMBERS: &[&str] = &[
    compact_properties::CONTEXT_SOURCE_EXTRAS,
    compact_properties::JSON,
    compact_properties::PREVIOUS_JSON,
    compact_properties::PROBLEM_DETAILS,
];

/// A `@context` written somewhere clause 5.5.7 does not admit one.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EmbeddedContextError {
    /// A `@context` stands below the top level of the document.
    #[error(
        "an @context is embedded under {}, and ETSI GS CIM 009 v1.9.1 clause 5.5.7 admits a user @context only at the top level of an NGSI-LD document",
        .path.join("/")
    )]
    BelowTopLevel {
        /// The member names leading from the document's top level down to the
        /// object that carries the embedded `@context`.
        path: Vec<String>,
    },
}

/// Refuse an embedded `@context` anywhere below the top level of an entity or
/// entity-shaped fragment.
///
/// A payload that is not an object is left to whoever validates its shape; an
/// array is the batch case, whose elements are each a document with a top level
/// of their own.
///
/// # Errors
///
/// Returns [`EmbeddedContextError::BelowTopLevel`] for the first `@context`
/// found below the top level, naming the member path that reaches it.
pub fn reject_embedded_contexts_in_fragment(fragment: &Value) -> Result<(), EmbeddedContextError> {
    let mut path = Vec::new();
    reject_in_top_level_slot(fragment, &mut path)
}

/// Refuse an embedded `@context` below the top level of the bare Attribute
/// Fragment of clauses 5.6.4 and 5.6.19, or of the single instance a clause
/// 5.6.14 modification carries.
///
/// The fragment's own top level is where a request written as
/// `application/ld+json` legitimately carries its `@context`, so only what
/// stands under one of the fragment's members is refused.
///
/// # Errors
///
/// Returns [`EmbeddedContextError::BelowTopLevel`] on the same terms as
/// [`reject_embedded_contexts_in_fragment`], with `attribute` heading the
/// reported path.
pub fn reject_embedded_contexts_in_attribute<'a>(attribute: &'a str, node: &'a Value) -> Result<(), EmbeddedContextError> {
    let mut path = vec![attribute];
    reject_in_top_level_slot(node, &mut path)
}

/// Walk one slot that stands at the document's top level: an object, or the
/// instances of a multi-instance array.
fn reject_in_top_level_slot<'a>(node: &'a Value, path: &mut Vec<&'a str>) -> Result<(), EmbeddedContextError> {
    match node {
        Value::Object(members) => reject_below_top_level(members, path),
        Value::Array(instances) => instances.iter().try_for_each(|instance| reject_in_top_level_slot(instance, path)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Walk the members of one top-level object, where a `@context` is the
/// document's own and is admitted.
fn reject_below_top_level<'a>(members: &'a Map<String, Value>, path: &mut Vec<&'a str>) -> Result<(), EmbeddedContextError> {
    members
        .iter()
        .filter(|(member, _)| member.as_str() != CONTEXT_MEMBER && !is_native_json_member(member))
        .try_for_each(|(member, value)| reject_in_slot(member, value, path))
}

/// Walk one member's value, at any depth below the top level.
fn reject_in_slot<'a>(member: &'a str, value: &'a Value, path: &mut Vec<&'a str>) -> Result<(), EmbeddedContextError> {
    match value {
        Value::Object(members) => {
            path.push(member);
            let outcome = reject_in_object(members, path);
            path.pop();
            outcome
        }
        Value::Array(items) => items.iter().try_for_each(|item| reject_in_slot(member, item, path)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Refuse an object below the top level that carries a `@context`, and descend
/// into everything it holds that is not a native JSON literal.
fn reject_in_object<'a>(members: &'a Map<String, Value>, path: &mut Vec<&'a str>) -> Result<(), EmbeddedContextError> {
    if members.contains_key(CONTEXT_MEMBER) {
        return Err(EmbeddedContextError::BelowTopLevel {
            path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        });
    }
    members
        .iter()
        .filter(|(member, _)| !is_native_json_member(member))
        .try_for_each(|(member, value)| reject_in_slot(member, value, path))
}

/// Whether a member's content is a native JSON literal the JSON-LD processor
/// never expands (clause 5.5.7, closing note).
fn is_native_json_member(member: &str) -> bool {
    NATIVE_JSON_MEMBERS.contains(&member)
}

#[cfg(test)]
mod tests {
    use crate::embedded_context_admission::{EmbeddedContextError, reject_embedded_contexts_in_attribute, reject_embedded_contexts_in_fragment};
    use serde_json::json;

    /// The member path an error names, for comparison against the expected one.
    fn refused_path(error: &EmbeddedContextError) -> &[String] {
        match error {
            EmbeddedContextError::BelowTopLevel { path } => path,
        }
    }

    /// Clause 5.5.7: "there cannot be parts of the user @context other than at
    /// the top level of the NGSI-LD document", so the document's own top-level
    /// `@context` is the one that stands.
    #[test]
    fn a_top_level_context_is_the_documents_own_and_is_admitted() {
        let fragment = json!({
            "@context": ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld", {"speed": "https://example.org/speed"}],
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 42}
        });
        assert!(reject_embedded_contexts_in_fragment(&fragment).is_ok());
    }

    #[test]
    fn a_context_embedded_in_an_attribute_node_is_refused_under_the_attribute_name() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"@context": {"speed": "https://example.org/speed"}, "type": "Property", "value": 42}
        });
        let error = reject_embedded_contexts_in_fragment(&fragment).expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_path(&error), ["speed".to_owned()]);
    }

    #[test]
    fn a_context_embedded_in_a_sub_attribute_is_refused_under_the_full_member_path() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 42,
                "accuracy": {"@context": {"accuracy": "https://example.org/accuracy"}, "type": "Property", "value": 0.5}
            }
        });
        let error = reject_embedded_contexts_in_fragment(&fragment).expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_path(&error), ["speed".to_owned(), "accuracy".to_owned()]);
    }

    /// A Property's `value` is expanded as JSON-LD, so a `@context` inside it
    /// is applied by the processor and is exactly the reshaping clause 5.5.7
    /// names. The seal walk of
    /// [`crate::attribute_type_validation`](crate::attribute_type_validation)
    /// stops at a value object; this one does not.
    #[test]
    fn a_context_inside_a_property_value_object_is_refused() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": {"@context": {"x": "https://example.org/x"}, "x": 1}}
        });
        let error = reject_embedded_contexts_in_fragment(&fragment).expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_path(&error), ["speed".to_owned(), "value".to_owned()]);
    }

    #[test]
    fn a_context_inside_a_geo_property_geometry_and_a_language_map_is_refused() {
        let geometry = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"@context": {}, "type": "Point", "coordinates": [1.0, 2.0]}}
        });
        assert!(reject_embedded_contexts_in_fragment(&geometry).is_err());

        let language_map = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "label": {"type": "LanguageProperty", "languageMap": {"@context": {}, "en": "hello"}}
        });
        assert!(reject_embedded_contexts_in_fragment(&language_map).is_err());
    }

    /// Clause 5.5.7, closing note: a `JsonProperty` "is designed to hold
    /// native JSON values which are by definition not available for expansion
    /// and compaction via an @context", so a key spelled `@context` inside its
    /// `json` member is data.
    #[test]
    fn a_context_key_inside_a_json_property_is_data_and_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "config": {"type": "JsonProperty", "json": {"@context": {"anything": "at all"}, "nested": {"@context": "https://example.org/ctx.jsonld"}}}
        });
        assert!(reject_embedded_contexts_in_fragment(&fragment).is_ok());
    }

    /// The core context coerces `previousJson`, `contextSourceExtras` and
    /// `problemDetails` to `@type: "@json"` exactly as it does `json`, so the
    /// same exemption reaches all four wherever they stand.
    #[test]
    fn every_core_native_json_member_is_exempt_at_any_depth() {
        for member in ["json", "previousJson", "contextSourceExtras", "problemDetails"] {
            let fragment = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "config": {"type": "JsonProperty", "json": {"ok": true}, member: {"@context": {}}}
            });
            assert!(reject_embedded_contexts_in_fragment(&fragment).is_ok(), "{member}");
        }
    }

    /// A `json` member standing inside a Property value is still the core
    /// `json` term, whose content the processor keeps verbatim.
    #[test]
    fn a_context_under_a_json_member_nested_in_a_value_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": {"json": {"@context": {}}}}
        });
        assert!(reject_embedded_contexts_in_fragment(&fragment).is_ok());
    }

    #[test]
    fn a_context_in_one_instance_of_a_multi_instance_attribute_is_refused() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:b", "@context": {}}
            ]
        });
        assert!(reject_embedded_contexts_in_fragment(&fragment).is_err());
    }

    /// A batch payload is an array of documents, each with a top level of its
    /// own where its `@context` legitimately stands.
    #[test]
    fn each_element_of_a_batch_array_keeps_its_own_top_level() {
        let admitted = json!([
            {"@context": {"speed": "https://example.org/speed"}, "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"},
            {"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle"}
        ]);
        assert!(reject_embedded_contexts_in_fragment(&admitted).is_ok());

        let refused = json!([
            {"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"},
            {"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "speed": {"type": "Property", "value": 1, "@context": {}}}
        ]);
        assert!(reject_embedded_contexts_in_fragment(&refused).is_err());
    }

    /// Clause 5.6.4 makes an Attribute Fragment the attribute node itself, so
    /// the `@context` an `application/ld+json` request writes beside it is at
    /// the document's top level.
    #[test]
    fn an_attribute_fragment_keeps_its_own_top_level_context() {
        let fragment = json!({"@context": {"speed": "https://example.org/speed"}, "type": "Property", "value": 42});
        assert!(reject_embedded_contexts_in_attribute("speed", &fragment).is_ok());
    }

    #[test]
    fn an_attribute_fragment_is_refused_for_a_context_below_its_top_level() {
        let fragment = json!({"type": "Property", "value": 42, "accuracy": {"type": "Property", "value": 0.5, "@context": {}}});
        let error = reject_embedded_contexts_in_attribute("speed", &fragment).expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_path(&error), ["speed".to_owned(), "accuracy".to_owned()]);

        let instances = json!([{"type": "Property", "value": 42, "unit": {"@context": {}}}]);
        assert!(reject_embedded_contexts_in_attribute("speed", &instances).is_err());
    }

    #[test]
    fn a_payload_that_is_not_an_object_carries_no_embedded_context() {
        for payload in [json!("not an object"), json!(42), json!(null), json!(true)] {
            assert!(reject_embedded_contexts_in_fragment(&payload).is_ok(), "{payload}");
        }
    }

    /// An ordinary entity, with every attribute family the eight of clause
    /// 5.2.4 admit, passes untouched.
    #[test]
    fn an_entity_carrying_no_embedded_context_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/a/b",
            "speed": {"type": "Property", "value": 42, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z"},
            "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
            "label": {"type": "LanguageProperty", "languageMap": {"en": "hello", "de": "hallo"}},
            "category": {"type": "VocabProperty", "vocab": "Truck"},
            "config": {"type": "JsonProperty", "json": {"a": [1, 2, {"b": true}]}},
            "readings": {"type": "ListProperty", "valueList": [1, 2, 3]},
            "sites": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Site:1"]}
        });
        assert!(reject_embedded_contexts_in_fragment(&fragment).is_ok());
    }
}
