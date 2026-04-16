//! Collation metadata augmentation and query rewriting (NGSI-LD Annex G.2).
//!
//! Injects a normalized `collate` sub-attribute into expanded Property values
//! so that language-insensitive queries can match on diacritic-stripped,
//! lowercased strings.  Also rewrites `q` filter expressions to target the
//! collation sub-attribute and normalize string comparison operands.

use crate::{
    core::error::AndromedaError,
    jsonld::vocab,
    query::q::{
        ast::{AttrPath, CompareValue, Expr, Predicate, Scalar},
        parser,
        serialize,
    },
};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

/// Full IRI for the Andromeda-internal `collate` sub-attribute.
const COLLATE_IRI: &str = "https://andromeda.internal/vocab/collate";

/// Injects a normalized `collate` sub-attribute for string Property values.
///
/// Operates on expanded JSON-LD (post-expansion, pre-storage). For every
/// Property whose `hasValue` is a string, a sub-attribute with the collation
/// IRI is inserted containing the NFD-normalized, mark-stripped, lowercased
/// form of the value.
pub fn augment_expanded_with_collation_metadata(expanded: &mut Value) {
    match expanded {
        Value::Array(items) => {
            for item in items {
                if let Some(object) = item.as_object_mut() {
                    augment_entity_object(object);
                }
            }
        }
        Value::Object(object) => augment_entity_object(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Rewrites `q` expressions to target `<attr>.collate` and normalize string RHS values.
///
/// For each attribute name in `targets`, appends the collation IRI as a
/// sub-attribute path segment and normalizes all string operands on the
/// right-hand side of comparisons.
pub fn rewrite_query_for_collation(q: Option<&str>, targets: &[String]) -> Result<Option<String>, AndromedaError> {
    let Some(q_str) = q else {
        return Ok(None);
    };
    if q_str.trim().is_empty() || targets.is_empty() {
        return Ok(Some(q_str.to_string()));
    }

    let target_set: HashSet<&str> = targets.iter().map(String::as_str).collect();
    let expr = parser::parse(q_str)?;
    let rewritten = rewrite_expr(expr, &target_set);
    Ok(Some(serialize::serialize(&rewritten)))
}

/// Produces a normalization suitable for language-insensitive matching.
///
/// Follows NGSI-LD v1.9.1 Annex G.2.1:
/// `str.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLower()`
///
/// Steps:
/// 1. NFD decomposition (separates base characters from combining marks)
/// 2. Remove combining marks (General Category M)
/// 3. Lowercase
/// 4. Collapse whitespace runs to single space
pub fn normalize_for_collation(value: &str) -> String {
    value
        .nfd()
        .filter(|c| {
            !matches!(
                get_general_category(*c),
                GeneralCategory::NonspacingMark | GeneralCategory::SpacingMark | GeneralCategory::EnclosingMark
            )
        })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn augment_entity_object(object: &mut Map<String, Value>) {
    let keys: Vec<String> = object.keys().cloned().collect();
    for key in keys {
        if key.starts_with('@') {
            continue;
        }
        if let Some(value) = object.get_mut(&key) {
            augment_attribute_value(value);
        }
    }
}

fn augment_attribute_value(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(object) = item.as_object_mut() {
                    augment_attribute_instance(object);
                }
            }
        }
        Value::Object(object) => augment_attribute_instance(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn augment_attribute_instance(instance: &mut Map<String, Value>) {
    if let Some(collated) = extract_string_property_value(instance).map(|value| normalize_for_collation(&value)) {
        instance.insert(COLLATE_IRI.to_string(), build_collate_sub_attribute(collated));
    }

    let keys: Vec<String> = instance.keys().cloned().collect();
    for key in keys {
        if key.starts_with('@') || key == vocab::expanded::VALUE.as_str() || key == vocab::expanded::OBJECT.as_str() || key == COLLATE_IRI {
            continue;
        }
        if let Some(value) = instance.get_mut(&key) {
            augment_attribute_value(value);
        }
    }
}

fn extract_string_property_value(instance: &Map<String, Value>) -> Option<String> {
    let attr_types = instance.get("@type")?.as_array()?;
    if !attr_types
        .iter()
        .filter_map(Value::as_str)
        .any(|value| value == vocab::expanded::PROPERTY.as_str())
    {
        return None;
    }

    extract_expanded_value_string(instance.get(vocab::expanded::VALUE.as_str())?)
}

fn build_collate_sub_attribute(value: String) -> Value {
    json!([{
        "@type": [vocab::expanded::PROPERTY.as_str()],
        (vocab::expanded::VALUE.as_str()): [{"@value": value}]
    }])
}

fn rewrite_expr(expr: Expr, targets: &HashSet<&str>) -> Expr {
    match expr {
        Expr::And(terms) => Expr::And(terms.into_iter().map(|term| rewrite_expr(term, targets)).collect()),
        Expr::Or(terms) => Expr::Or(terms.into_iter().map(|term| rewrite_expr(term, targets)).collect()),
        Expr::Pred(predicate) => Expr::Pred(rewrite_predicate(predicate, targets)),
    }
}

fn rewrite_predicate(predicate: Predicate, targets: &HashSet<&str>) -> Predicate {
    match predicate {
        Predicate::Exists(path) => Predicate::Exists(rewrite_attr_path(path, targets)),
        Predicate::Compare { path, op, rhs } => {
            let path = rewrite_attr_path(path, targets);
            let rhs = if path.sub_attrs.last().is_some_and(|segment| segment == COLLATE_IRI) {
                normalize_compare_value(rhs)
            } else {
                rhs
            };
            Predicate::Compare { path, op, rhs }
        }
        Predicate::MatchPattern { path, pattern } => Predicate::MatchPattern {
            path: rewrite_attr_path(path, targets),
            pattern: normalize_for_collation(&pattern),
        },
        Predicate::NotMatchPattern { path, pattern } => Predicate::NotMatchPattern {
            path: rewrite_attr_path(path, targets),
            pattern: normalize_for_collation(&pattern),
        },
    }
}

fn rewrite_attr_path(mut path: AttrPath, targets: &HashSet<&str>) -> AttrPath {
    if targets.contains(path.attr.as_str()) {
        path.sub_attrs.push(COLLATE_IRI.to_string());
    }
    path
}

fn normalize_compare_value(value: CompareValue) -> CompareValue {
    match value {
        CompareValue::Scalar(Scalar::Str(string)) => CompareValue::Scalar(Scalar::Str(normalize_for_collation(&string))),
        CompareValue::ValueList(values) => CompareValue::ValueList(
            values
                .into_iter()
                .map(|value| match value {
                    Scalar::Str(string) => Scalar::Str(normalize_for_collation(&string)),
                    other => other,
                })
                .collect(),
        ),
        CompareValue::Range { min, max } => CompareValue::Range {
            min: normalize_scalar(min),
            max: normalize_scalar(max),
        },
        other => other,
    }
}

fn normalize_scalar(value: Scalar) -> Scalar {
    match value {
        Scalar::Str(string) => Scalar::Str(normalize_for_collation(&string)),
        other => other,
    }
}

fn extract_expanded_value_string(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => {
            let values: Vec<String> = items.iter().filter_map(extract_single_value_string).collect();
            if values.is_empty() { None } else { Some(values.join(" ")) }
        }
        Value::Object(_) => extract_single_value_string(value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn extract_single_value_string(node: &Value) -> Option<String> {
    let object = node.as_object()?;
    match object.get("@value")? {
        Value::String(string) => Some(string.clone()),
        Value::Array(items) => {
            let values: Vec<String> = items.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect();
            if values.is_empty() { None } else { Some(values.join(" ")) }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn normalize_for_collation_removes_diacritics() {
        pretty_assertions::assert_eq!(normalize_for_collation("Schone Grusse"), "schone grusse");
        pretty_assertions::assert_eq!(normalize_for_collation("Schone  Grusse"), "schone grusse");
    }

    #[test]
    fn normalize_for_collation_strips_combining_marks() {
        // U+0308 = combining diaeresis; NFD of "o" = "o\u{0308}"
        pretty_assertions::assert_eq!(normalize_for_collation("Scho\u{0308}ne Gru\u{0308}sse"), "schone grusse");
    }

    #[test]
    fn normalize_for_collation_lowercases() {
        pretty_assertions::assert_eq!(normalize_for_collation("HELLO WORLD"), "hello world");
    }

    #[test]
    fn normalize_for_collation_collapses_whitespace() {
        pretty_assertions::assert_eq!(normalize_for_collation("a   b  c"), "a b c");
    }

    #[test]
    fn normalize_for_collation_handles_non_latin() {
        // CJK characters have no combining marks; pass through lowercased
        let result = normalize_for_collation("Tokyo");
        pretty_assertions::assert_eq!(result, "tokyo");
    }

    #[test]
    fn rewrite_query_for_collation_changes_targeted_attr() {
        let rewritten = rewrite_query_for_collation(Some("name==\"Schone Grusse\""), &[String::from("name")]).unwrap();
        pretty_assertions::assert_eq!(rewritten, Some(format!("name.\"{}\"==\"schone grusse\"", COLLATE_IRI)));
    }

    #[test]
    fn rewrite_query_for_collation_no_targets_passthrough() {
        let rewritten = rewrite_query_for_collation(Some("name==\"test\""), &[]).unwrap();
        pretty_assertions::assert_eq!(rewritten, Some("name==\"test\"".to_string()));
    }

    #[test]
    fn rewrite_query_for_collation_empty_q_passthrough() {
        let rewritten = rewrite_query_for_collation(Some("  "), &[String::from("name")]).unwrap();
        pretty_assertions::assert_eq!(rewritten, Some("  ".to_string()));
    }

    #[test]
    fn rewrite_query_for_collation_none_returns_none() {
        let rewritten = rewrite_query_for_collation(None, &[String::from("name")]).unwrap();
        pretty_assertions::assert_eq!(rewritten, None);
    }

    #[test]
    fn rewrite_query_for_collation_multiple_targets() {
        let rewritten = rewrite_query_for_collation(
            Some("name==\"test\";description==\"hello\""),
            &[String::from("name"), String::from("description")],
        )
        .unwrap();
        let result = rewritten.unwrap();
        assert!(result.contains(&format!("name.\"{}\"", COLLATE_IRI)));
        assert!(result.contains(&format!("description.\"{}\"", COLLATE_IRI)));
    }
}
