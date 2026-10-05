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

use serde_json::{Map, Value};
use thiserror::Error;

/// Why a JSON document carries no local context to reduce it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
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
pub fn context_subtree(document: Value) -> Result<Value, UnusableLocalContext> {
    let Value::Object(mut members) = document else {
        return Err(UnusableLocalContext::NotAnObject);
    };
    let subtree = members.remove("@context").ok_or(UnusableLocalContext::NoContextMember)?;
    let is_context = match &subtree {
        Value::Array(entries) => entries.iter().all(is_context_entry),
        Value::Null | Value::String(_) | Value::Object(_) => true,
        Value::Bool(_) | Value::Number(_) => false,
    };
    if !is_context {
        return Err(UnusableLocalContext::NotAContext);
    }
    Ok(Value::Object(Map::from_iter([("@context".to_owned(), subtree)])))
}

/// Whether `entry` may stand in a context array (JSON-LD 1.1 clause 9.15): a
/// `null`, a reference to a remote context, or a context definition, but never
/// another array.
const fn is_context_entry(entry: &Value) -> bool {
    match entry {
        Value::Null | Value::String(_) | Value::Object(_) => true,
        Value::Bool(_) | Value::Number(_) | Value::Array(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::context_subtree::{UnusableLocalContext, context_subtree};
    use serde_json::json;

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
            context_subtree(supplied),
            Ok(json!({ "@context": { "Building": "https://example.com/Building" } }))
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

            pretty_assertions::assert_eq!(context_subtree(supplied), Ok(json!({ "@context": subtree })));
        }
    }

    /// Clause 5.13.2.3 requires a top-level `@context` field; without one there
    /// is no local context at all, and each absence keeps its own reason.
    #[test]
    fn a_document_without_a_top_level_context_carries_no_local_context() {
        for (supplied, reason) in [
            (json!({ "A1": "urn:ngsi-ld:attributes:A1" }), UnusableLocalContext::NoContextMember),
            (json!({}), UnusableLocalContext::NoContextMember),
            (json!([{ "@context": {} }]), UnusableLocalContext::NotAnObject),
            (json!("@context"), UnusableLocalContext::NotAnObject),
            (json!(5), UnusableLocalContext::NotAnObject),
        ] {
            pretty_assertions::assert_eq!(context_subtree(supplied), Err(reason));
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
            pretty_assertions::assert_eq!(
                context_subtree(json!({ "@context": subtree })),
                Err(UnusableLocalContext::NotAContext),
                "{subtree} is not a context"
            );
        }
    }
}
