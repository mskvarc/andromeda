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
//! The top level of a document and the names of its attributes are held to the
//! attribute-name rules by the decode of the request, so the rules here read
//! the values standing below them: [`screen_keys_in_nested_value`] reads one
//! such value, located from itself.
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

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    embedded_context_admission::is_native_json_member,
};
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, JsonPointerSegment};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use jsonld_syntax::{is_keyword, is_keyword_like};
use thiserror::Error;

/// The members a node object declares its types in: the core `@context`'s
/// protected alias and the keyword itself.
const TYPE_MEMBERS: &[&str] = &[keyword::TYPE_ALIAS, keyword::TYPE];

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

/// Refuse the first key the rules do not admit in a value standing below the
/// top level of a document: the value of a typed attribute instance, read
/// apart from the document it was written in.
///
/// # Errors
///
/// Returns the [`CompactKeyError`] of the first key refused, located from the
/// value itself.
pub fn screen_keys_in_nested_value<'n, N: CompactNode<'n>>(value: N) -> Result<(), CompactKeyError> {
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

/// The pointer segment naming the member `name`.
fn member_segment(name: &str) -> JsonPointerSegment {
    JsonPointerSegment::Member(Box::from(name))
}

/// Walk one member's value, at any depth below the top level.
fn screen_member<'n, N: CompactNode<'n>>(member: &str, value: N, path: &mut Vec<JsonPointerSegment>) -> Result<(), CompactKeyError> {
    let reading = if LANGUAGE_MAP_MEMBERS.contains(&member) {
        KeyReading::LanguageTags
    } else {
        KeyReading::Terms
    };
    path.push(member_segment(member));
    let outcome = screen_slot(value, reading, path);
    path.pop();
    outcome
}

/// Walk one value below the top level: an object, or every element of an array.
fn screen_slot<'n, N: CompactNode<'n>>(value: N, reading: KeyReading, path: &mut Vec<JsonPointerSegment>) -> Result<(), CompactKeyError> {
    match value.shape() {
        NodeShape::Object(members) => screen_object(&members, reading, path),
        NodeShape::Array(items) => items.into_iter().enumerate().try_for_each(|(index, item)| {
            path.push(JsonPointerSegment::Index(index));
            let outcome = screen_slot(item, reading, path);
            path.pop();
            outcome
        }),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Geometry(_) => Ok(()),
    }
}

/// Refuse an object below the top level that carries a key the rules refuse,
/// and descend into everything it holds that is not a native JSON literal.
fn screen_object<'n, N: CompactNode<'n>>(members: &NodeMembers<'n, N>, reading: KeyReading, path: &mut Vec<JsonPointerSegment>) -> Result<(), CompactKeyError> {
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
fn screen_key<'n, N: CompactNode<'n>>(member: &str, value: N, path: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
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
fn names_an_empty_type<'n, N: CompactNode<'n>>(value: N) -> bool {
    match value.shape() {
        NodeShape::String(type_name) => type_name.is_empty(),
        NodeShape::Array(type_names) => type_names.into_iter().any(|type_name| type_name.text().is_some_and(|name| name.is_empty())),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => false,
    }
}

/// The pointer to the key `member` of the object standing at `path`.
fn pointer_to(path: &[JsonPointerSegment], member: &str) -> JsonPointer {
    JsonPointer::from_segments(path.iter().cloned().chain([member_segment(member)]).collect())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        compact_key_admission::{self, CompactKeyError},
        written_json::WrittenJson,
    };
    use serde_json::{Value, json};

    /// The nested-value screening on the text `value` is written as.
    fn screen_keys_in_nested_value(value: &Value) -> Result<(), CompactKeyError> {
        compact_key_admission::screen_keys_in_nested_value(WrittenJson::of(value).node())
    }

    /// The JSON Pointer a refusal names, for comparison
    /// against the expected one.
    fn refused_path(error: &CompactKeyError) -> String {
        match error {
            CompactKeyError::KeywordFormKey { path } | CompactKeyError::EmptyKey { path } | CompactKeyError::EmptyType { path } => path.to_string(),
        }
    }

    /// Clause 5.5.7, closing note: a `JsonProperty` "is designed to hold
    /// native JSON values which are by definition not available for expansion
    /// and compaction via an @context", so a key spelled `@context` inside a
    /// `json` member is data, as is any other key there.
    #[test]
    fn any_key_inside_a_json_member_is_data_and_is_admitted() {
        let value =
            json!({"json": {"@context": {"anything": "at all"}, "nested": {"@context": "https://example.org/ctx.jsonld"}, "@foo": 1, "": {"type": ""}}});
        assert!(screen_keys_in_nested_value(&value).is_ok());
    }

    /// The core context coerces `previousJson`, `contextSourceExtras` and
    /// `problemDetails` to `@type: "@json"` exactly as it does `json`, so the
    /// same exemption reaches all four at any depth.
    #[test]
    fn every_core_native_json_member_is_exempt_at_any_depth() {
        for member in ["json", "previousJson", "contextSourceExtras", "problemDetails"] {
            let value = json!({"outer": {member: {"@foo": {"": 1}}}});
            assert!(screen_keys_in_nested_value(&value).is_ok(), "{member}");
        }
    }

    /// JSON-LD 1.1 expansion ignores a key that has the form of a keyword
    /// without being one, so the member would vanish (clause 4.6.4).
    #[test]
    fn a_keyword_form_key_is_refused_at_every_depth() {
        for (value, path) in [(json!({"@foo": "x", "name": "y"}), "/@foo"), (json!([{"a": {"@Bar": 1}}]), "/0/a/@Bar")] {
            let error = screen_keys_in_nested_value(&value).expect_err("a keyword-form key is refused");
            assert!(matches!(error, CompactKeyError::KeywordFormKey { .. }), "{error:?}");
            pretty_assertions::assert_eq!(refused_path(&error), path);
        }
    }

    /// A key starting with `@` that is no keyword form (it holds more than
    /// letters) is an ordinary term expansion keeps.
    #[test]
    fn a_key_starting_with_an_at_sign_but_holding_more_than_letters_is_admitted() {
        assert!(screen_keys_in_nested_value(&json!({"@foo-bar": 1, "@1": 2})).is_ok());
    }

    /// The empty key and the empty type would come back as the `@vocab` IRI
    /// compacted, `ngsi-ld:default-context/` (clause 4.6.4).
    #[test]
    fn an_empty_key_or_type_is_refused() {
        let error = screen_keys_in_nested_value(&json!({"": "x"})).expect_err("an empty key is refused");
        assert!(matches!(error, CompactKeyError::EmptyKey { .. }), "{error:?}");
        pretty_assertions::assert_eq!(refused_path(&error), "/");

        for value in [json!({"type": ""}), json!({"@type": ["A", ""]})] {
            let error = screen_keys_in_nested_value(&value).expect_err("an empty type is refused");
            assert!(matches!(error, CompactKeyError::EmptyType { .. }), "{error:?}");
        }
    }

    /// The keys of a language map are language tags, which the language rules
    /// read; neither key rule applies to them.
    #[test]
    fn the_keys_of_a_language_map_are_left_to_the_language_rules() {
        assert!(screen_keys_in_nested_value(&json!({"languageMap": {"": "x", "@foo": "y"}})).is_ok());
    }

    /// A value read apart from its document is located from the value itself.
    #[test]
    fn a_nested_value_is_screened_from_its_own_top() {
        let error = screen_keys_in_nested_value(&json!({"a": {"": 1}})).expect_err("an empty key is refused");
        pretty_assertions::assert_eq!(refused_path(&error), "/a/");
    }
}
