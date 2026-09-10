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

/// The `@context` subtree of `document` on its own.
///
/// Returns `None` when `document` is not a JSON object, or is one carrying no
/// top-level `@context` member: neither is a local context, so there is nothing
/// to reduce and nothing to store.
#[must_use]
pub fn context_subtree(document: Value) -> Option<Value> {
    let Value::Object(mut members) = document else {
        return None;
    };
    let subtree = members.remove("@context")?;
    Some(Value::Object(Map::from_iter([("@context".to_owned(), subtree)])))
}

#[cfg(test)]
mod tests {
    use crate::context_subtree::context_subtree;
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
            Some(json!({ "@context": { "Building": "https://example.com/Building" } }))
        );
    }

    /// Clause 5.13.2.3 keeps the subtree itself whole, whatever JSON-LD shape a
    /// local context takes: an object, an array of references, or one string.
    #[test]
    fn every_shape_of_context_subtree_survives_intact() {
        for subtree in [
            json!({ "Building": "https://example.com/Building" }),
            json!(["https://example.com/a.jsonld", { "b": "https://example.com/b" }]),
            json!("https://example.com/only.jsonld"),
        ] {
            let supplied = json!({ "@context": subtree, "discarded": true });

            pretty_assertions::assert_eq!(context_subtree(supplied), Some(json!({ "@context": subtree })));
        }
    }

    /// Clause 5.13.2.3 requires a top-level `@context` field; without one there
    /// is no local context at all.
    #[test]
    fn a_document_without_a_top_level_context_carries_no_local_context() {
        for supplied in [
            json!({ "A1": "urn:ngsi-ld:attributes:A1" }),
            json!({}),
            json!([{ "@context": {} }]),
            json!("@context"),
        ] {
            pretty_assertions::assert_eq!(context_subtree(supplied), None);
        }
    }
}
