//! The rule by which a broker answer is judged to match an annex C example.
//!
//! The rule itself is stated in this suite's crate-level documentation. This
//! module implements it and nothing else: it normalizes both sides and then
//! asserts exact deep equality, so a difference the rule does not explicitly
//! forgive is a test failure.

use crate::example_context::canonicalize_example_urls;
use serde_json::Value;

/// Members the broker mints at write time, which no printed example can state.
const BROKER_MINTED: [&str; 4] = ["createdAt", "modifiedAt", "deletedAt", "instanceId"];

/// Whether the compared payload's `@context` member takes part in the comparison.
///
/// Clauses C.7, C.8 and C.9 are about what the rendered `@context` contains, so
/// their tests compare it. Everywhere else the printed `@context` is the
/// example author's choice rather than a statement about broker behaviour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextMember {
    /// Strip `@context` from both sides before comparing.
    Ignored,
    /// Keep `@context` and compare it like any other member.
    Compared,
}

/// Whether the top level of the payload is an unordered collection of Entities.
///
/// Clause 5.7.2 mandates no order for a query answer unless `orderBy` was
/// given, so a test replaying a query example sorts both sides by `id` first.
/// Every other array keeps its printed order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityOrder {
    /// The answer is an unordered set of Entities; sort by `id` before comparing.
    Unordered,
    /// The answer's array order is meaningful and is compared as printed.
    AsPrinted,
}

/// Assert that `actual` is the payload annex C prints as `expected`.
///
/// `clause` names the example so a failure reads as a clause reference.
pub fn assert_matches_example(clause: &str, expected: &Value, actual: &Value, context: ContextMember, order: EntityOrder) {
    let expected = normalize(expected, context, order);
    let actual = normalize(actual, context, order);
    pretty_assertions::assert_eq!(expected, actual, "annex C clause {clause} example does not match the broker answer");
}

/// Apply every normalization the comparison rule allows.
fn normalize(payload: &Value, context: ContextMember, order: EntityOrder) -> Value {
    let payload = canonicalize_example_urls(payload);
    let payload = strip(&payload, context);
    let payload = canonical_numbers(&payload);
    match order {
        EntityOrder::Unordered => sort_entities(payload),
        EntityOrder::AsPrinted => payload,
    }
}

/// Render every number in its shortest exact decimal form.
///
/// Clause 4.7.2 calls `GeoJSON` coordinates floating-point values, so `100` and
/// `100.0` name the same coordinate; `serde_json` holds them as different
/// variants. Comparing numbers by value rather than by variant is what the
/// comparison rule means by "numbers compare by JSON value", and it keeps the
/// rule from failing an answer over a distinction JSON does not draw.
fn canonical_numbers(payload: &Value) -> Value {
    match payload {
        Value::Number(number) => number
            .as_f64()
            .map_or_else(|| payload.clone(), |value| Value::String(format!("number:{value}"))), // clone: a number JSON cannot widen keeps its own form.
        Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.clone(), canonical_numbers(value))) // clone: the normalized copy owns its keys.
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(canonical_numbers).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::String(_)) => other.clone(), // clone: the normalized copy owns its leaves.
    }
}

/// Remove broker-minted members, and `@context` when it is not being compared.
fn strip(payload: &Value, context: ContextMember) -> Value {
    match payload {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(key, _)| !BROKER_MINTED.contains(&key.as_str()))
                .filter(|(key, _)| context == ContextMember::Compared || key.as_str() != "@context")
                .map(|(key, value)| (key.clone(), strip(value, context))) // clone: the normalized copy owns its keys.
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|item| strip(item, context)).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other.clone(), // clone: the normalized copy owns its leaves.
    }
}

/// Sort a top-level array of Entities by `id`, leaving every other shape alone.
fn sort_entities(payload: Value) -> Value {
    match payload {
        Value::Array(mut entities) => {
            entities.sort_by_key(entity_sort_key);
            Value::Array(entities)
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other,
    }
}

/// The identity a query answer's member is ordered by.
///
/// A `GeoJSON` `Feature` and an NGSI-LD Entity both carry `id`; anything without
/// one keeps a stable position relative to its peers by sorting under the empty
/// key, which leaves the printed order of unidentified members intact.
fn entity_sort_key(entity: &Value) -> String {
    entity.get("id").and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Parse one corpus document, failing loudly on a transcription slip.
pub fn corpus(document: &str) -> Value {
    serde_json::from_str(document).expect("annex C corpus document is valid JSON")
}

/// Self-tests for the comparison rule itself.
///
/// The rule decides what every other test in this suite can and cannot detect,
/// so it is pinned here rather than trusted: a rule that quietly stopped
/// noticing an extra member would turn the whole corpus green.
mod rule {
    use crate::comparison::{ContextMember, EntityOrder, assert_matches_example, normalize};
    use serde_json::json;

    #[test]
    fn comparison_rule_erases_broker_minted_timestamps_at_every_depth() {
        let actual = json!({
            "id": "urn:ngsi-ld:Vehicle:A4567",
            "createdAt": "2026-01-01T00:00:00Z",
            "brandName": {"type": "Property", "value": "Mercedes", "modifiedAt": "2026-01-01T00:00:00Z"}
        });
        let expected = json!({
            "id": "urn:ngsi-ld:Vehicle:A4567",
            "brandName": {"type": "Property", "value": "Mercedes"}
        });
        assert_matches_example("test", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);
    }

    #[test]
    fn comparison_rule_does_not_forgive_an_unexpected_extra_member() {
        let expected = normalize(&json!({"id": "urn:ngsi-ld:Vehicle:A4567"}), ContextMember::Ignored, EntityOrder::AsPrinted);
        let actual = normalize(
            &json!({"id": "urn:ngsi-ld:Vehicle:A4567", "scope": "/Madrid"}),
            ContextMember::Ignored,
            EntityOrder::AsPrinted,
        );
        assert_ne!(expected, actual, "an extra member has to fail the comparison");
    }

    #[test]
    fn comparison_rule_treats_an_integer_and_its_float_spelling_as_one_number() {
        let expected = json!({"coordinates": [100.0, 0.0]});
        let actual = json!({"coordinates": [100, 0]});
        assert_matches_example("test", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);
    }

    #[test]
    fn comparison_rule_still_separates_two_different_numbers() {
        let expected = normalize(&json!({"value": 121}), ContextMember::Ignored, EntityOrder::AsPrinted);
        let actual = normalize(&json!({"value": 122}), ContextMember::Ignored, EntityOrder::AsPrinted);
        assert_ne!(expected, actual, "two different numbers have to fail the comparison");
    }

    #[test]
    fn comparison_rule_compares_a_query_answer_without_regard_to_entity_order() {
        let expected = json!([{"id": "urn:ngsi-ld:Vehicle:A456"}, {"id": "urn:ngsi-ld:Vehicle:B9211"}]);
        let actual = json!([{"id": "urn:ngsi-ld:Vehicle:B9211"}, {"id": "urn:ngsi-ld:Vehicle:A456"}]);
        assert_matches_example("test", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);
    }

    #[test]
    fn comparison_rule_keeps_the_value_list_order_the_example_prints() {
        let expected = normalize(&json!({"valueList": [300, 120]}), ContextMember::Ignored, EntityOrder::AsPrinted);
        let actual = normalize(&json!({"valueList": [120, 300]}), ContextMember::Ignored, EntityOrder::AsPrinted);
        assert_ne!(expected, actual, "a reordered valueList has to fail the comparison");
    }

    #[test]
    fn comparison_rule_compares_the_context_member_only_when_the_clause_is_about_it() {
        let expected = json!({"id": "urn:ngsi-ld:Vehicle:A4567", "@context": ["http://example.org/a.jsonld"]});
        let actual = json!({"id": "urn:ngsi-ld:Vehicle:A4567", "@context": ["http://example.org/b.jsonld"]});
        assert_matches_example("test", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);
        assert_ne!(
            normalize(&expected, ContextMember::Compared, EntityOrder::AsPrinted),
            normalize(&actual, ContextMember::Compared, EntityOrder::AsPrinted),
            "a differing @context has to fail when the clause is about the @context"
        );
    }
}
