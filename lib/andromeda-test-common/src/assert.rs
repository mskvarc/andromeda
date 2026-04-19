//! Shared assertion helpers.
//!
//! Test files import these via the crate prelude so per-suite assertion
//! logic stays uniform — a single `assert_problem_details` handles both the
//! RFC 7807 envelope shape and the NGSI-LD `type` URI.

use pretty_assertions::assert_eq;
use serde_json::Value;

/// Assert that `body` is a Problem-Details object whose `type` field equals
/// `expected_type`.
///
/// Panics with a `pretty_assertions` diff when the assertion fails.
pub fn assert_problem_details(body: &Value, expected_type: &str) {
    let actual_type = body.get("type").and_then(Value::as_str).unwrap_or("<missing type>");
    assert_eq!(actual_type, expected_type, "Problem-Details type mismatch: body = {body}");
    assert!(body.get("title").and_then(Value::as_str).is_some(), "Problem-Details missing `title`: {body}");
}

/// Assert that `body` is an NGSI-LD Entity whose `id` equals `expected_id`.
pub fn assert_entity_id(body: &Value, expected_id: &str) {
    let actual = body.get("id").and_then(Value::as_str).unwrap_or("<missing id>");
    assert_eq!(actual, expected_id, "entity id mismatch");
}
