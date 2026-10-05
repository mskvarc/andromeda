//! Refusing the keys a compact NGSI-LD document cannot be written with: a key
//! JSON-LD expansion would drop or rewrite past recovery.
//!
//! A `@context` below the top level of a document is the other key clause
//! 5.5.7 refuses; it is a property of the request's user `@context`, found on
//! the request text by `crate::embedded_context_admission` and refused by the
//! user `@context` admission before any payload rule here is reached.
//!
//! Clause 4.6.4 requires a broker to "preserve the representation of the
//! content of the values provided by the context information providers and
//! return the original content". Two kinds of key cannot be, whatever the
//! request's contexts say, so they are refused rather than silently changed:
//!
//! - a key that has the form of a JSON-LD keyword (`@` followed by letters)
//!   without being one, which JSON-LD 1.1 expansion ignores, so the member and
//!   everything under it would vanish;
//! - an empty key, or an empty `type`, inside an attribute: JSON-LD 1.1 forbids
//!   defining the empty string as a term, so IRI expansion turns it into the
//!   `@vocab` IRI itself, and IRI compaction returns a vocabulary-relative
//!   suffix only when it is not empty (JSON-LD 1.1 API, IRI compaction step
//!   4.1), so it would come back as `ngsi-ld:default-context/`.
//!
//! An empty key at the top level of a document or an attribute fragment names
//! an attribute, which the attribute-name rules refuse, so only the keyword
//! form is checked there.
//!
//! The rules are decidable on the compact payload a client wrote and on nothing
//! else. The walk is unconditional -- every object below a top-level member is
//! examined, including a Property's `value`, a `GeoProperty`'s geometry and a
//! `LanguageProperty`'s `languageMap`, all of which the JSON-LD processor
//! traverses. The keys of a language map are
//! language tags rather than terms (a `@language` container), so they are read
//! by the language rules instead of these.
//!
//! The one exemption is a native JSON literal (see
//! `crate::embedded_context_admission::is_native_json_member`): the processor
//! never expands inside one, so any key there is data.
//!
//! This is a statement about a representation, which is why it is stated here
//! rather than beside one write path. Which of the write paths run it is
//! decided by those paths: a payload a Context Source or another broker sent
//! back is not a user `@context` and is never screened.

use crate::embedded_context_admission::is_native_json_member;
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, PathSegment};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use jsonld_syntax::{is_keyword, is_keyword_like};
use serde_json::{Map, Value};
use thiserror::Error;

/// The members a node object declares its types in: the core `@context`'s
/// protected alias and the keyword itself.
const TYPE_MEMBERS: &[&str] = &["type", "@type"];

/// The compacted members the NGSI-LD Core `@context` declares as language
/// maps, whose keys are language tags rather than terms.
const LANGUAGE_MAP_MEMBERS: &[&str] = &[compact_properties::LANGUAGE_MAP, compact_properties::LANGUAGE_MAPS];

/// A key a compact document cannot be written with.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompactKeyError {
    /// A key has the form of a JSON-LD keyword without being one (clause
    /// 4.6.4).
    #[error(
        "the key at {path} has the form of a JSON-LD keyword without being one; JSON-LD expansion drops such a key, so it could not be returned as written"
    )]
    KeywordFormKey {
        /// Where the key stands, the key itself last.
        path: JsonPointer,
    },
    /// A key inside an attribute is empty (clause 4.6.4).
    #[error("the key at {path} is empty; JSON-LD expansion turns it into the @vocab IRI itself, which compaction cannot return as an empty key")]
    EmptyKey {
        /// Where the key stands, the key itself last.
        path: JsonPointer,
    },
    /// A `type` inside an attribute is empty (clause 4.6.4).
    #[error("the type at {path} is empty; JSON-LD expansion turns it into the @vocab IRI itself, which compaction cannot return as an empty type")]
    EmptyType {
        /// Where the `type` member stands.
        path: JsonPointer,
    },
}

impl CompactKeyError {
    /// The same refusal located by `relocate`, for a walk handed one part of a
    /// document that reports from that part.
    #[must_use]
    pub fn relocated(self, relocate: impl FnOnce(&JsonPointer) -> JsonPointer) -> CompactKeyError {
        match self {
            CompactKeyError::KeywordFormKey { path } => CompactKeyError::KeywordFormKey { path: relocate(&path) },
            CompactKeyError::EmptyKey { path } => CompactKeyError::EmptyKey { path: relocate(&path) },
            CompactKeyError::EmptyType { path } => CompactKeyError::EmptyType { path: relocate(&path) },
        }
    }
}

/// Refuse the first key the rules do not admit anywhere in an entity or
/// entity-shaped fragment.
///
/// A payload that is not an object is left to whoever validates its shape; an
/// array is the batch case, whose elements are each a document with a top level
/// of their own.
///
/// # Errors
///
/// Returns the [`CompactKeyError`] of the first key refused, in member order.
pub fn screen_keys_in_fragment(fragment: &Value) -> Result<(), CompactKeyError> {
    let mut path = Vec::new();
    screen_top_level_slot(fragment, &mut path)
}

/// Refuse the first key the rules do not admit in the bare Attribute Fragment
/// of clauses 5.6.4 and 5.6.19, or in the single instance a clause 5.6.14
/// modification carries.
///
/// The fragment's own top level is where a request written as
/// `application/ld+json` legitimately carries its `@context`.
///
/// # Errors
///
/// Returns the [`CompactKeyError`] of the first key refused, with `attribute`
/// heading the reported location.
pub fn screen_keys_in_attribute<'a>(attribute: &'a str, node: &'a Value) -> Result<(), CompactKeyError> {
    let mut path = vec![PathSegment::Member(attribute)];
    screen_top_level_slot(node, &mut path)
}

/// Refuse the first key the rules do not admit in a value standing below the
/// top level of a document: the value of a typed attribute instance, read
/// apart from the document it was written in.
///
/// # Errors
///
/// Returns the [`CompactKeyError`] of the first key refused, located from the
/// value itself.
pub fn screen_keys_in_nested_value(value: &Value) -> Result<(), CompactKeyError> {
    let mut path = Vec::new();
    screen_slot(value, KeyReading::Terms, &mut path)
}

/// How the keys of an object are read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyReading {
    /// As terms JSON-LD expansion turns into IRIs.
    Terms,
    /// As the language tags of a language map.
    LanguageTags,
}

/// Walk one slot that stands at the document's top level: an object, or the
/// instances of a multi-instance array.
fn screen_top_level_slot<'a>(node: &'a Value, path: &mut Vec<PathSegment<'a>>) -> Result<(), CompactKeyError> {
    match node {
        Value::Object(members) => screen_top_level_object(members, path),
        Value::Array(instances) => instances.iter().enumerate().try_for_each(|(index, instance)| {
            path.push(PathSegment::Index(index));
            let outcome = screen_top_level_slot(instance, path);
            path.pop();
            outcome
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Walk the members of one top-level object.
fn screen_top_level_object<'a>(members: &'a Map<String, Value>, path: &mut Vec<PathSegment<'a>>) -> Result<(), CompactKeyError> {
    members.iter().try_for_each(|(member, value)| {
        if is_keyword_form(member) {
            return Err(CompactKeyError::KeywordFormKey {
                path: pointer_to(path, member),
            });
        }
        if is_native_json_member(member) {
            return Ok(());
        }
        screen_member(member, value, path)
    })
}

/// Walk one member's value, at any depth below the top level.
fn screen_member<'a>(member: &'a str, value: &'a Value, path: &mut Vec<PathSegment<'a>>) -> Result<(), CompactKeyError> {
    let reading = if LANGUAGE_MAP_MEMBERS.contains(&member) {
        KeyReading::LanguageTags
    } else {
        KeyReading::Terms
    };
    path.push(PathSegment::Member(member));
    let outcome = screen_slot(value, reading, path);
    path.pop();
    outcome
}

/// Walk one value below the top level: an object, or every element of an array.
fn screen_slot<'a>(value: &'a Value, reading: KeyReading, path: &mut Vec<PathSegment<'a>>) -> Result<(), CompactKeyError> {
    match value {
        Value::Object(members) => screen_object(members, reading, path),
        Value::Array(items) => items.iter().enumerate().try_for_each(|(index, item)| {
            path.push(PathSegment::Index(index));
            let outcome = screen_slot(item, reading, path);
            path.pop();
            outcome
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// Refuse an object below the top level that carries a key the rules refuse,
/// and descend into everything it holds that is not a native JSON literal.
fn screen_object<'a>(members: &'a Map<String, Value>, reading: KeyReading, path: &mut Vec<PathSegment<'a>>) -> Result<(), CompactKeyError> {
    if reading == KeyReading::Terms {
        members.iter().try_for_each(|(member, value)| screen_key(member, value, path))?;
    }
    members
        .iter()
        .filter(|(member, _)| !is_native_json_member(member))
        .try_for_each(|(member, value)| screen_member(member, value, path))
}

/// Refuse one key of an object below the top level, read as a term, that
/// expansion would drop or rewrite past recovery.
fn screen_key(member: &str, value: &Value, path: &[PathSegment<'_>]) -> Result<(), CompactKeyError> {
    if is_keyword_form(member) {
        return Err(CompactKeyError::KeywordFormKey {
            path: pointer_to(path, member),
        });
    }
    if member.is_empty() {
        return Err(CompactKeyError::EmptyKey {
            path: pointer_to(path, member),
        });
    }
    if TYPE_MEMBERS.contains(&member) && names_an_empty_type(value) {
        return Err(CompactKeyError::EmptyType {
            path: pointer_to(path, member),
        });
    }
    Ok(())
}

/// Whether a key has the form of a JSON-LD keyword without being one (JSON-LD
/// 1.1 section 9.1), which expansion ignores.
fn is_keyword_form(member: &str) -> bool {
    is_keyword_like(member) && !is_keyword(member)
}

/// Whether a `type` member's value is, or holds, the empty string.
fn names_an_empty_type(value: &Value) -> bool {
    match value {
        Value::String(type_name) => type_name.is_empty(),
        Value::Array(type_names) => type_names.iter().any(|type_name| type_name.as_str() == Some("")),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
    }
}

/// The pointer to the key `member` of the object standing at `path`.
fn pointer_to(path: &[PathSegment<'_>], member: &str) -> JsonPointer {
    let mut walk = path.to_vec();
    walk.push(PathSegment::Member(member));
    JsonPointer::from_walk(&walk)
}

#[cfg(test)]
mod tests {
    use crate::compact_key_admission::{CompactKeyError, screen_keys_in_attribute, screen_keys_in_fragment, screen_keys_in_nested_value};
    use serde_json::json;

    /// The JSON Pointer a refusal names, for comparison
    /// against the expected one.
    fn refused_path(error: &CompactKeyError) -> String {
        match error {
            CompactKeyError::KeywordFormKey { path } | CompactKeyError::EmptyKey { path } | CompactKeyError::EmptyType { path } => path.to_string(),
        }
    }

    /// Clause 5.5.7, closing note: a `JsonProperty` "is designed to hold
    /// native JSON values which are by definition not available for expansion
    /// and compaction via an @context", so a key spelled `@context` inside its
    /// `json` member is data, as is any other key there.
    #[test]
    fn any_key_inside_a_json_property_is_data_and_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "config": {"type": "JsonProperty", "json": {"@context": {"anything": "at all"}, "nested": {"@context": "https://example.org/ctx.jsonld"}, "@foo": 1, "": {"type": ""}}}
        });
        assert!(screen_keys_in_fragment(&fragment).is_ok());
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
                "config": {"type": "JsonProperty", "json": {"ok": true}, member: {"@foo": {"": 1}}}
            });
            assert!(screen_keys_in_fragment(&fragment).is_ok(), "{member}");
        }
    }

    /// A `json` member standing inside a Property value is still the core
    /// `json` term, whose content the processor keeps verbatim.
    #[test]
    fn a_keyword_form_key_under_a_json_member_nested_in_a_value_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": {"json": {"@foo": {}}}}
        });
        assert!(screen_keys_in_fragment(&fragment).is_ok());
    }

    #[test]
    fn a_payload_that_is_not_an_object_carries_no_key_to_refuse() {
        for payload in [json!("not an object"), json!(42), json!(null), json!(true)] {
            assert!(screen_keys_in_fragment(&payload).is_ok(), "{payload}");
        }
    }

    /// An ordinary entity, with every attribute family the eight of clause
    /// 5.2.4 admit, passes untouched; so does a structured value whose keys
    /// are no attribute names, which clause 4.6.2 does not restrict.
    #[test]
    fn an_entity_carrying_no_refused_key_is_admitted() {
        let fragment = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/a/b",
            "speed": {"type": "Property", "value": 42, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z"},
            "address": {"type": "Property", "value": {"opening hours": "9-5", "id": "abc", "type": "Postal Address", "@id": "x", "@type": "y"}},
            "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
            "label": {"type": "LanguageProperty", "languageMap": {"en": "hello", "de": "hallo", "@none": "hi"}},
            "category": {"type": "VocabProperty", "vocab": "Truck"},
            "config": {"type": "JsonProperty", "json": {"a": [1, 2, {"b": true}]}},
            "readings": {"type": "ListProperty", "valueList": [1, 2, 3]},
            "sites": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Site:1"]}
        });
        assert!(screen_keys_in_fragment(&fragment).is_ok());
    }

    /// JSON-LD 1.1 expansion ignores a key that has the form of a keyword
    /// without being one, so the member would vanish (clause 4.6.4).
    #[test]
    fn a_keyword_form_key_is_refused_at_every_depth() {
        for (payload, path) in [
            (json!({"id": "urn:x:1", "type": "T", "@foo": 1}), "/@foo"),
            (
                json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": 1, "@foo": 1}}),
                "/p/@foo",
            ),
            (
                json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": {"@foo": "x", "name": "y"}}}),
                "/p/value/@foo",
            ),
            (
                json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": [{"a": {"@Bar": 1}}]}}),
                "/p/value/0/a/@Bar",
            ),
        ] {
            let error = screen_keys_in_fragment(&payload).expect_err("a keyword-form key is refused");
            assert!(matches!(error, CompactKeyError::KeywordFormKey { .. }), "{error:?}");
            pretty_assertions::assert_eq!(refused_path(&error), path);
        }
    }

    /// A key starting with `@` that is no keyword form (it holds more than
    /// letters) is an ordinary term expansion keeps.
    #[test]
    fn a_key_starting_with_an_at_sign_but_holding_more_than_letters_is_admitted() {
        let payload = json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": {"@foo-bar": 1, "@1": 2}}});
        assert!(screen_keys_in_fragment(&payload).is_ok());
    }

    /// The empty key and the empty type would come back as the `@vocab` IRI
    /// compacted, `ngsi-ld:default-context/` (clause 4.6.4).
    #[test]
    fn an_empty_key_or_type_inside_an_attribute_is_refused() {
        let empty_key = json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": {"": "x"}}});
        let error = screen_keys_in_fragment(&empty_key).expect_err("an empty key is refused");
        assert!(matches!(error, CompactKeyError::EmptyKey { .. }), "{error:?}");
        pretty_assertions::assert_eq!(refused_path(&error), "/p/value/");

        for value in [json!({"type": ""}), json!({"@type": ["A", ""]})] {
            let empty_type = json!({"id": "urn:x:1", "type": "T", "p": {"type": "Property", "value": value}});
            let error = screen_keys_in_fragment(&empty_type).expect_err("an empty type is refused");
            assert!(matches!(error, CompactKeyError::EmptyType { .. }), "{error:?}");
        }
    }

    /// The keys of a language map are language tags, which the language rules
    /// read; neither key rule applies to them.
    #[test]
    fn the_keys_of_a_language_map_are_left_to_the_language_rules() {
        let payload = json!({"id": "urn:x:1", "type": "T", "label": {"type": "LanguageProperty", "languageMap": {"": "x", "@foo": "y"}}});
        assert!(screen_keys_in_fragment(&payload).is_ok());
    }

    /// A bare Attribute Fragment reports a refused key under the attribute
    /// name the request URI carries, through its instances too.
    #[test]
    fn an_attribute_fragment_is_located_under_its_attribute_name() {
        let fragment = json!({"type": "Property", "value": 42, "accuracy": {"type": "Property", "value": 0.5, "@foo": 1}});
        let error = screen_keys_in_attribute("speed", &fragment).expect_err("a keyword-form key is refused");
        pretty_assertions::assert_eq!(refused_path(&error), "/speed/accuracy/@foo");

        let instances = json!([{"type": "Property", "value": 42, "unit": {"": 1}}]);
        let error = screen_keys_in_attribute("speed", &instances).expect_err("an empty key is refused");
        pretty_assertions::assert_eq!(refused_path(&error), "/speed/0/unit/");
    }

    /// A value read apart from its document is located from the value itself.
    #[test]
    fn a_nested_value_is_screened_from_its_own_top() {
        let error = screen_keys_in_nested_value(&json!({"a": {"": 1}})).expect_err("an empty key is refused");
        pretty_assertions::assert_eq!(refused_path(&error), "/a/");
    }
}
