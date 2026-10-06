//! Reducing a JSON-LD document to the local context it carries.
//!
//! Clause 5.13.2.3 defines a stored `@context` as "a JSON object that has a
//! top-level field named @context, i.e. a JSON object representing a JSON-LD
//! 'local context'", and requires that "all extra information located outside
//! of the @context subtree in the referenced object shall be discarded", as the
//! JSON-LD specification specifies.
//!
//! That obligation is not particular to the Add @context operation. It is a
//! property of a *referenced* context object, so it holds at every point a
//! context document enters the broker: a client handing one over to be hosted,
//! and the resolver downloading one to cache. The reduction therefore lives
//! here, once, rather than at each entry point where it could be forgotten —
//! and the resolver's copy was forgotten, which is why a downloaded document's
//! unrelated members used to be stored beside its `@context`.

use andromeda_json_literal::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use std::sync::Arc;
use thiserror::Error;

/// Why a JSON document carries no local context to reduce it to.
#[derive(Debug, Clone, Error)]
pub enum UnusableLocalContext {
    /// The document is valid JSON but not a JSON object.
    #[error("the document is not a JSON object")]
    NotAnObject,
    /// The document is a JSON object without a top-level `@context` member.
    #[error("the document has no top-level @context member")]
    NoContextMember,
    /// The top-level `@context` member holds no JSON-LD context.
    ///
    /// JSON-LD 1.1 clause 9.15: a context is `null`, a string, an object, or an
    /// array whose every element is one of those three.
    #[error("the top-level @context member is not a JSON-LD context")]
    NotAContext,
    /// The subtree could not be written as a document of its own, which the
    /// subtree of an admitted document never fails to be.
    #[error("the @context subtree could not be written as a document of its own: {0}")]
    Unwritable(#[source] Arc<JsonLiteralError>),
}

/// The `@context` subtree of `document` on its own.
///
/// # Errors
///
/// Returns [`UnusableLocalContext::NotAnObject`] when `document` is not a JSON
/// object, [`UnusableLocalContext::NoContextMember`] when it is one carrying no
/// top-level `@context` member, and [`UnusableLocalContext::NotAContext`] when
/// that member holds a value no JSON-LD context takes: none of them is a local
/// context, so there is nothing to reduce and nothing to store.
pub fn context_subtree(document: &JsonLiteral) -> Result<JsonLiteral, UnusableLocalContext> {
    let members = document.piece().members().ok_or(UnusableLocalContext::NotAnObject)?;
    let subtree = members
        .iter()
        .rev()
        .find_map(|(key, value)| (key == "@context").then_some(*value))
        .ok_or(UnusableLocalContext::NoContextMember)?;
    let is_context = match subtree.kind() {
        JsonKind::Array => subtree.elements().unwrap_or_default().into_iter().all(is_context_entry),
        JsonKind::Null | JsonKind::String | JsonKind::Object => true,
        JsonKind::Boolean | JsonKind::Number => false,
    };
    if !is_context {
        return Err(UnusableLocalContext::NotAContext);
    }
    if members.len() == 1 {
        // clone: the reduced document is a copy the caller owns
        return Ok(document.clone());
    }
    local_context_document(subtree).map_err(|error| UnusableLocalContext::Unwritable(Arc::new(error)))
}

/// The document `{"@context": context}`: the shape clause 5.13.2.3 gives every
/// stored `@context`.
///
/// # Errors
///
/// Returns the [`JsonLiteralError`] the assembled text is refused with, which
/// the piece of an admitted literal never causes.
pub fn local_context_document(context: JsonPiece<'_>) -> Result<JsonLiteral, JsonLiteralError> {
    JsonLiteral::admit(&format!("{{\"@context\":{}}}", context.text().trim()))
}

/// Whether `entry` may stand in a context array (JSON-LD 1.1 clause 9.15): a
/// `null`, a reference to a remote context, or a context definition, but never
/// another array.
fn is_context_entry(entry: JsonPiece<'_>) -> bool {
    match entry.kind() {
        JsonKind::Null | JsonKind::String | JsonKind::Object => true,
        JsonKind::Boolean | JsonKind::Number | JsonKind::Array => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::context_subtree::{UnusableLocalContext, context_subtree};
    use andromeda_json_literal::JsonLiteral;
    use serde_json::json;

    /// The reduction of `supplied`, read back as JSON.
    fn reduced(supplied: &serde_json::Value) -> Result<serde_json::Value, UnusableLocalContext> {
        let supplied = JsonLiteral::admit(&supplied.to_string()).expect("an admissible document");
        context_subtree(&supplied).map(|reduced| serde_json::from_str(reduced.text()).expect("JSON"))
    }

    /// Clause 5.13.2.3: "all extra information located outside of the @context
    /// subtree in the referenced object shall be discarded".
    #[test]
    fn members_outside_the_context_subtree_are_discarded() {
        let supplied = json!({
            "@context": { "Building": "https://example.com/Building" },
            "id": "urn:ngsi-ld:Context:from-the-origin-server",
            "unrelated": [1, 2, 3]
        });

        pretty_assertions::assert_eq!(
            reduced(&supplied).expect("a local context"),
            json!({ "@context": { "Building": "https://example.com/Building" } })
        );
    }

    /// Clause 5.13.2.3 keeps the subtree itself whole, whatever JSON-LD shape a
    /// local context takes: an object, an array of references, one string, or
    /// the `null` that clears the active context.
    #[test]
    fn every_shape_of_context_subtree_survives_intact() {
        for subtree in [
            json!({ "Building": "https://example.com/Building" }),
            json!(["https://example.com/a.jsonld", { "b": "https://example.com/b" }, null]),
            json!("https://example.com/only.jsonld"),
            json!(null),
            json!([]),
        ] {
            let supplied = json!({ "@context": subtree, "discarded": true });

            pretty_assertions::assert_eq!(reduced(&supplied).expect("a local context"), json!({ "@context": subtree }));
        }
    }

    /// Clause 5.13.2.3 requires a top-level `@context` field; without one there
    /// is no local context at all, and each absence keeps its own reason.
    #[test]
    fn a_document_without_a_top_level_context_carries_no_local_context() {
        for supplied in [json!({ "A1": "urn:ngsi-ld:attributes:A1" }), json!({})] {
            assert!(matches!(reduced(&supplied), Err(UnusableLocalContext::NoContextMember)), "{supplied}");
        }
        for supplied in [json!([{ "@context": {} }]), json!("@context"), json!(5)] {
            assert!(matches!(reduced(&supplied), Err(UnusableLocalContext::NotAnObject)), "{supplied}");
        }
    }

    /// JSON-LD 1.1 clause 9.15: a context is `null`, a string, an object or an
    /// array of those; a top-level `@context` holding anything else is no local
    /// context, however well-formed the JSON around it.
    #[test]
    fn a_context_member_holding_no_context_carries_no_local_context() {
        for subtree in [
            json!(5),
            json!(true),
            json!(["https://example.com/a.jsonld", 5]),
            json!([false]),
            json!([["https://example.com/nested.jsonld"]]),
        ] {
            assert!(
                matches!(reduced(&json!({ "@context": subtree })), Err(UnusableLocalContext::NotAContext)),
                "{subtree} is not a context"
            );
        }
    }
}
