//! Finding a user `@context` embedded below the top level of an NGSI-LD
//! document.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "be embedded into NGSI-LD Attributes, i.e. there cannot be parts of the
//! user @context other than at the top level of the NGSI-LD document", and that
//! "parts of user @context that are not following the two points above should
//! result in an error of type `BadRequestData`, because JSON-LD Scoped Contexts
//! and nested embedded @context could be used to modify terms defined in the
//! Core @context or to reshape NGSI-LD Elements during the expansion of terms".
//!
//! The rule holds for every NGSI-LD document a client sends -- an Entity, a
//! Subscription, a Context Source Registration, a Query, an Entity Map or
//! Snapshot update -- so it is decided once, on the request text, by one
//! streaming pass that builds nothing: the members of the root object, or of
//! each element of a root array (a batch, whose elements are each a document
//! with a top level of their own), may carry `@context`; any object deeper down
//! that carries one is the finding. Which operations refuse the finding is the
//! user `@context` admission's decision, not this module's.
//!
//! The one exemption is a native JSON literal. Clause 5.5.7 closes with "note
//! that the `JsonProperty` is designed to hold native JSON values which are by
//! definition not available for expansion and compaction via an @context.
//! Therefore, the given short-hand name is always used for accessing JSON keys
//! within a `JsonProperty` json element": the processor never expands inside
//! one, so a key spelled `@context` there is data.

use andromeda_ngsi_ld_number::json_value::{
    json_pointer::{JsonPointer, JsonPointerSegment},
    value_census::MemberName,
};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::{cell::RefCell, fmt};
use thiserror::Error;

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

/// A user `@context` standing below the top level of an NGSI-LD document
/// (clause 5.5.7).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("an @context is embedded at {path}, but a user @context is admitted only at the top level of an NGSI-LD document")]
pub struct EmbeddedContext {
    /// Where the object that carries the embedded `@context` stands, from the
    /// document's top level down.
    pub path: JsonPointer,
}

/// Find the first `@context` embedded below the top level of the JSON text
/// `text`, in document order.
///
/// # Errors
///
/// Returns the streaming reader's failure for a text that is not one JSON
/// value, trailing content included.
pub fn find_embedded_context(text: &[u8]) -> Result<Option<EmbeddedContext>, serde_json::Error> {
    let walk = EmbeddedWalk::default();
    let mut reader = JsonReader::from_slice(text);
    EmbeddedSeed { walk: &walk }.deserialize(&mut reader).and_then(|()| reader.end())?;
    Ok(walk.found.into_inner())
}

/// Whether a member's content is a native JSON literal the JSON-LD processor
/// never expands (clause 5.5.7, closing note).
#[must_use]
pub fn is_native_json_member(member: &str) -> bool {
    NATIVE_JSON_MEMBERS.contains(&member)
}

/// The state of one walk: where it stands, and the first finding it met.
#[derive(Debug, Default)]
struct EmbeddedWalk {
    /// The location of the value being read, root first.
    path: RefCell<Vec<JsonPointerSegment>>,
    /// The first embedded `@context` met.
    found: RefCell<Option<EmbeddedContext>>,
}

impl EmbeddedWalk {
    /// Whether the object being read stands at a document's top level: the
    /// root, or an element of a root array.
    fn at_top_level(&self) -> bool {
        matches!(self.path.borrow().as_slice(), [] | [JsonPointerSegment::Index(_)])
    }

    /// Record the object being read as carrying an embedded `@context`, unless
    /// an earlier finding stands.
    fn find(&self) {
        let mut found = self.found.borrow_mut();
        if found.is_none() {
            // clone: the finding owns the location the walk goes on to change
            *found = Some(EmbeddedContext {
                path: JsonPointer::from_segments(self.path.borrow().clone()),
            });
        }
    }

    /// Read the value at `segment` below the current location with `read`.
    fn below<T, E>(&self, segment: JsonPointerSegment, read: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.path.borrow_mut().push(segment);
        let read = read();
        self.path.borrow_mut().pop();
        read
    }
}

/// Reads one value and everything below it, looking for an embedded
/// `@context`.
#[derive(Clone, Copy, Debug)]
struct EmbeddedSeed<'w> {
    /// The walk this value belongs to.
    walk: &'w EmbeddedWalk,
}

impl<'de> DeserializeSeed<'de> for EmbeddedSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for EmbeddedSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        let mut index = 0;
        while self.walk.below(JsonPointerSegment::Index(index), || items.next_element_seed(self))?.is_some() {
            index += 1;
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        let Some(first) = members.next_key_seed(MemberNameText)? else {
            return Ok(());
        };
        // The reader's spelling of one number, not an object.
        if MemberName::of(&first) == MemberName::NumberToken {
            members.next_value::<IgnoredAny>()?;
            return Ok(());
        }
        let top_level = self.walk.at_top_level();
        let mut name = first;
        loop {
            if &*name == keyword::CONTEXT && !top_level {
                self.walk.find();
            }
            if is_native_json_member(&name) {
                members.next_value::<IgnoredAny>()?;
            } else {
                self.walk.below(JsonPointerSegment::Member(name), || members.next_value_seed(self))?;
            }
            match members.next_key_seed(MemberNameText)? {
                Some(next) => name = next,
                None => return Ok(()),
            }
        }
    }
}

/// Reads one member name as owned text.
#[derive(Clone, Copy, Debug)]
struct MemberNameText;

impl<'de> DeserializeSeed<'de> for MemberNameText {
    type Value = Box<str>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Box<str>, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for MemberNameText {
    type Value = Box<str>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<Box<str>, E> {
        Ok(Box::from(name))
    }
}

#[cfg(test)]
mod tests {
    use crate::embedded_context_admission::find_embedded_context;
    use serde_json::{Value, json};

    /// Where the first embedded `@context` of `document` stands, if any.
    fn finding(document: &Value) -> Option<String> {
        find_embedded_context(document.to_string().as_bytes())
            .expect("valid JSON")
            .map(|found| found.path.to_string())
    }

    #[test]
    fn a_context_at_the_root_object_is_admitted() {
        assert_eq!(finding(&json!({"@context": {"a": "https://example.org/a"}, "id": "urn:ngsi-ld:T:1"})), None);
    }

    #[test]
    fn a_context_embedded_in_an_entity_is_found_where_it_stands() {
        for (document, path) in [
            (json!({"speed": {"type": "Property", "value": 1, "@context": {}}}), "/speed"),
            (
                json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 0.1, "@context": {}}}}),
                "/speed/accuracy",
            ),
            (json!({"speed": {"type": "Property", "value": {"@context": {}, "n": 1}}}), "/speed/value"),
            (
                json!({"location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2], "@context": {}}}}),
                "/location/value",
            ),
            (
                json!({"label": {"type": "LanguageProperty", "languageMap": {"en": "a", "@context": {}}}}),
                "/label/languageMap",
            ),
            (
                json!({"speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 2, "@context": {}}]}),
                "/speed/1",
            ),
        ] {
            assert_eq!(finding(&document).as_deref(), Some(path), "{document}");
        }
    }

    /// The elements of a root array are each a document with a top level of
    /// their own.
    #[test]
    fn a_batch_element_carries_its_own_context_and_a_nested_one_is_found() {
        let batch = json!([
            {"@context": "https://example.org/a", "id": "urn:ngsi-ld:T:1"},
            {"@context": "https://example.org/a", "id": "urn:ngsi-ld:T:2", "speed": {"type": "Property", "value": 1, "@context": {}}}
        ]);
        assert_eq!(finding(&batch).as_deref(), Some("/1/speed"));
    }

    #[test]
    fn a_context_embedded_in_a_subscription_or_registration_is_found() {
        assert_eq!(
            finding(&json!({"type": "Subscription", "entities": [{"type": "Device", "@context": {"Device": "https://example.org/Device"}}]})).as_deref(),
            Some("/entities/0")
        );
        assert_eq!(
            finding(&json!({"type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Device", "@context": {}}]}]})).as_deref(),
            Some("/information/0/entities/0")
        );
    }

    /// Clause 5.5.7, closing note: native JSON is data at any depth.
    #[test]
    fn a_context_key_inside_native_json_is_data() {
        for member in ["json", "previousJson", "contextSourceExtras", "problemDetails"] {
            let document = json!({"config": {"type": "JsonProperty", member: {"deep": {"@context": "https://example.org/x"}}}});
            assert_eq!(finding(&document), None, "{member}");
        }
        assert_eq!(
            finding(&json!({"speed": {"type": "Property", "value": {"json": {"@context": {}}}}})),
            None,
            "a json member inside a value is native JSON"
        );
    }

    /// An arbitrary-precision number reaches the walk as the reader's own
    /// object, which is no object of the document.
    #[test]
    fn an_arbitrary_precision_number_is_not_an_object() {
        let text = br#"{"speed": {"type": "Property", "value": 0.100000000000000005, "n": [18446744073709551616]}}"#;
        assert_eq!(find_embedded_context(text).expect("valid JSON"), None);
    }

    #[test]
    fn a_payload_that_is_no_object_has_no_finding() {
        for document in [json!(null), json!(7), json!("text"), json!(["urn:ngsi-ld:T:1"])] {
            assert_eq!(finding(&document), None, "{document}");
        }
    }

    #[test]
    fn the_first_finding_in_document_order_wins() {
        let document = json!({"a": {"type": "Property", "value": 1, "@context": {}}, "b": {"type": "Property", "value": 1, "@context": {}}});
        assert_eq!(finding(&document).as_deref(), Some("/a"));
    }

    #[test]
    fn a_text_that_is_not_json_is_refused() {
        assert!(find_embedded_context(b"[1,").is_err());
    }
}
