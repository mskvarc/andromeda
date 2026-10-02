//! The JSON tree an admitted request body text stands for.
//!
//! A body is admitted from its text and decoded straight from it; the few
//! readers that need it as a JSON tree -- a handler keeping the body untyped,
//! and the retracing decode naming the member a failed decoding stopped at --
//! parse it here, into the tree a body admitted as a tree always was: its
//! numbers in the spelling [`JsonNumber`](andromeda_ngsi_ld_number::JsonNumber)
//! renders, its top-level `@context` removed.

use andromeda_ngsi_ld_number::json_value::{canonicalize_numbers, number_range::NumberRangeError};
use serde_json::Value;

/// The member a JSON-LD document names its context in.
const CONTEXT_MEMBER: &str = "@context";

/// The JSON tree of an admitted body text: its numbers in canonical spelling,
/// its top-level `@context` removed.
///
/// # Errors
///
/// Returns the reader's failure or the refused number, neither of which a text
/// that passed the admission draws.
pub(crate) fn admitted_document(text: &[u8]) -> Result<Value, NumberRangeError> {
    let mut document: Value = serde_json::from_slice(text)?;
    canonicalize_numbers(&mut document)?;
    strip_context(&mut document);
    Ok(document)
}

/// Remove the top-level `@context` members of a document, or of each object
/// item of an array document.
fn strip_context(document: &mut Value) {
    match document {
        Value::Object(members) => {
            members.remove(CONTEXT_MEMBER);
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(members) = item {
                    members.remove(CONTEXT_MEMBER);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::admitted_document::admitted_document;
    use serde_json::json;

    #[test]
    fn the_context_is_removed_and_the_numbers_respelled() {
        let document = admitted_document(br#"{"@context":"https://example.org/a","n":[1.50,1E+2]}"#).expect("an admitted text");
        pretty_assertions::assert_eq!(document.to_string(), json!({"n": [1.5, 100]}).to_string());
        let items = admitted_document(br#"[{"@context":"x","id":"a"},7]"#).expect("an admitted text");
        pretty_assertions::assert_eq!(items, json!([{"id": "a"}, 7]));
    }
}
