//! The spellings of a document the specification reads the same, which a typed body may change: an origin URL with
//! or without its `/` path (RFC 3986 section 6.2.3), a single temporal instance or a one-element array of it (ETSI GS
//! CIM 009 v1.9.1 clauses 4.5.7 and 4.5.8), and an `objectList` item as a string or as `{"object": ...}` (clause
//! 5.2.37).

use crate::sequence::{
    body::{CONTEXT, Document},
    written_json::read,
};
use serde_json::{Map, Value as Json};

/// Whether the JSON text `rendering` is the document `written`, up to the spellings read the same.
pub(crate) fn reads_as(document: Document, rendering: &str, written: &Json) -> bool {
    read(rendering).is_ok_and(|rendered| equivalent(document, &rendered, written))
}

/// Whether `rendered` is `written`, up to the spellings read the same.
pub(crate) fn equivalent(document: Document, rendered: &Json, written: &Json) -> bool {
    let temporal = matches!(document, Document::Temporal | Document::TemporalAttributes);
    rendered == written || *rendered == canonical(written, if temporal { Depth::Top } else { Depth::Nested })
}

/// Whether a value is the top of a temporal document, whose attributes may hold a single instance, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// The temporal document itself.
    Top,
    /// A member or item of it, or a document that is not temporal.
    Nested,
}

/// `written` with each spelling the crate writes differently replaced by the crate's: an origin URL gains its `/`
/// path, an `objectList` string item becomes `{"object": ...}`, and an attribute of a temporal document holding one
/// instance becomes a one-element array.
fn canonical(written: &Json, depth: Depth) -> Json {
    match written {
        Json::String(text) if is_origin(text) => Json::String(format!("{text}/")),
        Json::Array(items) => Json::Array(items.iter().map(|item| canonical(item, Depth::Nested)).collect()),
        Json::Object(members) => Json::Object(
            members
                .iter()
                .map(|(name, value)| {
                    let value = match (name.as_str(), value) {
                        ("objectList", Json::Array(items)) => Json::Array(
                            items
                                .iter()
                                .map(|item| match item {
                                    Json::String(object) => {
                                        let mut wrapped = Map::new();
                                        wrapped.insert("object".to_owned(), Json::String(object.clone()));
                                        Json::Object(wrapped)
                                    }
                                    other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::Array(_) | Json::Object(_)) => other.clone(),
                                })
                                .collect(),
                        ),
                        ("id" | "type" | CONTEXT | "scope", other) => other.clone(),
                        (_, Json::Object(instance)) if depth == Depth::Top && is_instance(instance) => Json::Array(vec![Json::Object(instance.clone())]),
                        (_, other) => other.clone(),
                    };
                    (name.clone(), canonical(&value, Depth::Nested))
                })
                .collect(),
        ),
        other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_)) => other.clone(),
    }
}

/// Whether a member of a temporal document is a single attribute instance.
fn is_instance(members: &Map<String, Json>) -> bool {
    members.get("type").and_then(Json::as_str).is_some_and(|kind| {
        matches!(
            kind,
            "Property" | "Relationship" | "GeoProperty" | "LanguageProperty" | "VocabProperty" | "JsonProperty" | "ListProperty" | "ListRelationship"
        )
    })
}

/// Whether `text` is an origin: a scheme and an authority with no path.
fn is_origin(text: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        text.strip_prefix(scheme)
            .is_some_and(|authority| !authority.is_empty() && !authority.contains(['/', '?', '#']))
    })
}
