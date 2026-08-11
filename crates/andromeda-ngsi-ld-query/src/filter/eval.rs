//! In-memory evaluation of parsed `q` filter expressions against expanded JSON-LD entities and canonical-keyed Context Source properties.

use crate::filter::{
    expression::Expr,
    path::{AttrPath, QueryPath},
    predicate::{CmpOp, Predicate, SetMembership},
    scalar::{CompareValue, Scalar, parse_date, parse_date_time, parse_time},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{ENTITY, JSON, LANGUAGE_MAP, OBJECT, OBJECT_LIST, VALUE, VALUE_LIST, VOCAB};
use serde_json::{Map, Value};
use std::{cmp::Ordering, slice::from_ref};

/// Evaluate a parsed `q` expression against an expanded JSON-LD entity.
///
/// The input may be a single expanded JSON-LD object, its one-element array document, or canonical-keyed Context Source properties in normalized or scalar registration form.
pub fn evaluate(expr: &Expr, entity: &Value) -> bool {
    let obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return false,
    };
    let Some(obj) = obj else { return false };
    eval_expr(expr, obj)
}

/// Evaluate the boolean structure of an expression.
fn eval_expr(expr: &Expr, obj: &Map<String, Value>) -> bool {
    match expr {
        Expr::And(terms) => terms.iter().all(|t| eval_expr(t, obj)),
        Expr::Or(terms) => terms.iter().any(|t| eval_expr(t, obj)),
        Expr::Pred(pred) => eval_predicate(pred, obj),
    }
}

/// One resolved comparison target: the value the predicate compares, and
/// whether it is a Relationship's object.
///
/// Clause 4.9 forbids the ordering operators against a Relationship, so the
/// evaluator has to know a value's origin, not only its JSON shape — a
/// Relationship object and a string Property value are both JSON strings.
struct ResolvedTarget {
    /// The value to compare against the operand.
    value: Value,
    /// Whether it came from `hasObject`/`hasObjectList` (a Relationship object).
    is_relationship: bool,
}

/// Evaluate one leaf predicate.
fn eval_predicate(pred: &Predicate, obj: &Map<String, Value>) -> bool {
    match pred {
        // Clause 4.9: existence holds when the entity contains the target
        // element, whatever attribute family it belongs to.
        Predicate::Exists(path) => !resolve_targets(obj, path).is_empty(),
        Predicate::Equality { path, membership, rhs } => resolve_targets(obj, path).iter().any(|target| eval_equality(&target.value, *membership, rhs)),
        Predicate::Ordering { path, op, rhs } => resolve_targets(obj, path).iter().any(|target| {
            // Clause 4.9: an ordering comparison against a Relationship shall
            // not match — a URI has no `<`/`>` order the query can mean.
            if target.is_relationship {
                return false;
            }
            eval_cmp_scalar(&target.value, op.as_cmp_op(), rhs)
        }),
        // Clause 4.9: a pattern operator applies only to a String target; a
        // non-String value is considered as not matching, so a number or
        // boolean is never stringified into a match.
        Predicate::MatchPattern { path, pattern } => resolve_targets(obj, path)
            .iter()
            .filter_map(|target| target.value.as_str())
            .any(|value| pattern.is_match(value)),
        // `notPatternOp` also requires the entity to *contain* the target
        // element (clause 4.9); an entity that lacks the attribute, or carries
        // only non-String values, does not match.
        Predicate::NotMatchPattern { path, pattern } => resolve_targets(obj, path)
            .iter()
            .filter_map(|target| target.value.as_str())
            .any(|value| !pattern.is_match(value)),
    }
}

/// The expanded value members a `q` target can terminate at: the member IRI,
/// the leaf key that carries the value inside it, and whether the member
/// denotes a Relationship object.
///
/// Covers all eight attribute families of clause 5.2.4, not only Property and
/// Relationship, so a `q` over a LanguageProperty, VocabProperty, JsonProperty
/// or a list attribute resolves rather than silently missing.
fn value_members() -> [(&'static str, &'static str, bool); 7] {
    [
        (VALUE.as_str(), "@value", false),
        (OBJECT.as_str(), "@id", true),
        (VOCAB.as_str(), "@id", false),
        (LANGUAGE_MAP.as_str(), "@value", false),
        (JSON.as_str(), "@value", false),
        (VALUE_LIST.as_str(), "@value", false),
        (OBJECT_LIST.as_str(), "@id", true),
    ]
}

/// Resolve every value an attribute axis addresses, descending each
/// linked-entity hop into the entity it names.
///
/// A hop can only be followed when the document already carries the linked
/// entity: clause 4.5.23.2 `join=inline` nests it under the Relationship
/// instance's `entity` member, and that embedded copy is the only linked data
/// an in-memory evaluation has. Without it the hop resolves no target and the
/// predicate does not hold. That is not a weaker reading of clause 4.9 — for a
/// stored entity the SQL compilation is what answers a traversal filter, and it
/// reads the linked entity from the store. This evaluator only ever sees the one
/// document it is handed, whether that is a notification payload being matched
/// against a subscription or a residual filter finishing what a store declined.
fn resolve_targets(obj: &Map<String, Value>, path: &QueryPath) -> Vec<ResolvedTarget> {
    match path {
        QueryPath::Local(local) => resolve_values(obj, local),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => resolve_across_hop(obj, relationship, guard_types, inner),
    }
}

/// Resolve the inner axis against every entity embedded under a Relationship's
/// instances, keeping only those the hop's type guard admits.
fn resolve_across_hop(obj: &Map<String, Value>, relationship: &AttributeName, guard_types: &[AttributeName], inner: &QueryPath) -> Vec<ResolvedTarget> {
    let Some(attribute) = obj.get(relationship.as_str()) else {
        return Vec::new();
    };
    instances_of(attribute)
        .iter()
        .flat_map(|instance| instance.get(ENTITY.as_str()).into_iter().flat_map(|embedded| instances_of(embedded).iter()))
        .filter_map(Value::as_object)
        .filter(|linked| admitted_by_guard(linked, guard_types))
        .flat_map(|linked| resolve_targets(linked, inner))
        .collect()
}

/// The instances a member holds: an expanded JSON-LD member is an array, while a
/// canonical-keyed Context Source property is the bare node.
fn instances_of(member: &Value) -> &[Value] {
    match member.as_array() {
        Some(instances) => instances.as_slice(),
        None => from_ref(member),
    }
}

/// Whether a linked entity satisfies a hop's entity-type guard.
///
/// An empty guard admits every linked entity; otherwise clause 4.9's
/// `1*(EntityType)` list is a disjunction, so one shared type is enough.
fn admitted_by_guard(linked: &Map<String, Value>, guard_types: &[AttributeName]) -> bool {
    if guard_types.is_empty() {
        return true;
    }
    let Some(declared) = linked.get("@type") else {
        return false;
    };
    instances_of(declared)
        .iter()
        .filter_map(Value::as_str)
        .any(|declared| guard_types.iter().any(|guard| guard.as_str() == declared))
}

/// Resolve every value an attribute path addresses in an expanded JSON-LD
/// entity object or a canonical-keyed Context Source property map.
///
/// Navigates `obj[attr]` -> each instance -> sub-attrs -> the family's value
/// member, then the optional value path for compound values.
///
/// All instances are returned, not just the first. Clause 4.9 (p. 86) defines
/// the comparison target for a multi-instance attribute as "any Value of such
/// instances", so a predicate holds when it holds for at least one of them.
/// Restricting this to the first instance made a `datasetId`-partitioned
/// attribute match on whichever instance happened to serialise first.
fn resolve_values(obj: &Map<String, Value>, path: &AttrPath) -> Vec<ResolvedTarget> {
    let Some(attribute) = obj.get(path.attr.as_str()) else {
        return Vec::new();
    };
    if let Some(instances) = attribute.as_array() {
        if path.match_all_languages {
            return instances.iter().flat_map(|instance| resolve_all_languages(instance, path)).collect();
        }
        return instances.iter().filter_map(|instance| resolve_instance_value(instance, path)).collect();
    }
    if path.match_all_languages {
        resolve_all_languages(attribute, path)
    } else {
        resolve_instance_value(attribute, path).into_iter().collect()
    }
}

/// Resolve the `[*]` language-suppression form: every language's value of a
/// LanguageProperty instance (clause 4.9), so a predicate holds when it holds
/// for any language rather than only the one negotiation would select.
///
/// Falls back to the ordinary single-value resolution when the instance is not
/// a LanguageProperty, so `attr[*]` on another family still addresses its value.
fn resolve_all_languages(instance: &Value, path: &AttrPath) -> Vec<ResolvedTarget> {
    let mut current = instance;
    for sub in &path.sub_attrs {
        let Some(next) = current.get(sub.as_str()).and_then(Value::as_array).and_then(|values| values.first()) else {
            return Vec::new();
        };
        current = next;
    }
    if let Some(entries) = current.get(LANGUAGE_MAP.as_str()).and_then(Value::as_array) {
        return entries
            .iter()
            .filter_map(|entry| entry.get("@value"))
            // clone: the caller owns each resolved value beyond the instance borrow
            .map(|value| ResolvedTarget {
                value: value.clone(),
                is_relationship: false,
            })
            .collect();
    }
    resolve_instance_value(instance, path).into_iter().collect()
}

/// Resolve one attribute instance down to the value the predicate compares.
fn resolve_instance_value(instance: &Value, path: &AttrPath) -> Option<ResolvedTarget> {
    // Navigate sub-attributes (e.g. temperature.observedAt).
    let mut current = instance;
    for sub in &path.sub_attrs {
        current = current.get(sub.as_str())?.as_array()?.first()?;
    }

    let (raw, is_relationship) = terminal_value(current)?;

    // Navigate compound value path (e.g. address[city]).
    let mut value = raw;
    for key in &path.value_path {
        value = value.get(key)?;
    }
    // clone: the caller owns the resolved value, which outlives the borrow of `instance`
    Some(ResolvedTarget {
        value: value.clone(),
        is_relationship,
    })
}

/// Read an instance node down to the value its attribute family carries, and
/// whether that value is a Relationship object.
///
/// Tries each family's value member in turn, then a direct `@value`/`@id`
/// terminal — the shape a system sub-attribute such as `observedAt` or
/// `datasetId` takes, which carries neither `hasValue` nor `hasObject`.
fn terminal_value(node: &Value) -> Option<(&Value, bool)> {
    for (member, terminal_key, is_relationship) in value_members() {
        if let Some(first) = node.get(member).and_then(Value::as_array).and_then(|values| values.first())
            && let Some(raw) = first.get(terminal_key)
        {
            return Some((raw, is_relationship));
        }
    }
    if let Some(raw) = node.get("@value") {
        return Some((raw, false));
    }
    if let Some(raw) = node.get("@id") {
        return Some((raw, false));
    }
    if let Some(raw) = node.get("value") {
        return Some((raw, false));
    }
    match node {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Some((node, false)),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Evaluate an equality comparison (`==`/`!=`) against a scalar, a range, or a
/// value list.
///
/// "Outside" is the exact complement of "within" — for a range as for a value
/// list — so a target incomparable with the bounds is outside the range, not
/// neither in nor out.
fn eval_equality(entity_val: &Value, membership: SetMembership, rhs: &CompareValue) -> bool {
    match rhs {
        CompareValue::Scalar(scalar) => eval_cmp_scalar(entity_val, membership.as_cmp_op(), scalar),
        CompareValue::Range { min, max } => {
            let within = eval_cmp_scalar(entity_val, CmpOp::GreaterEq, min) && eval_cmp_scalar(entity_val, CmpOp::LessEq, max);
            match membership {
                SetMembership::Within => within,
                SetMembership::Outside => !within,
            }
        }
        CompareValue::ValueList(values) => {
            let any_match = values.iter().any(|v| eval_cmp_scalar(entity_val, CmpOp::Equal, v));
            match membership {
                SetMembership::Within => any_match,
                SetMembership::Outside => !any_match,
            }
        }
    }
}

/// Compare one entity value against one scalar, treating an unusable pairing as
/// a non-match.
///
/// An array target is compared by membership (clause 4.9): `==` holds when the
/// operand is one of its elements, `!=` when it is not, and an ordering
/// comparison against an array does not hold. Two values of different types are
/// considered unequal, so an incomparable pairing satisfies `!=` and fails `==`
/// and every ordering comparison.
fn eval_cmp_scalar(entity_val: &Value, op: CmpOp, scalar: &Scalar) -> bool {
    if let Value::Array(elements) = entity_val {
        let contains = elements.iter().any(|element| eval_cmp_scalar(element, CmpOp::Equal, scalar));
        return match op {
            CmpOp::Equal => contains,
            CmpOp::Unequal => !contains,
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => false,
        };
    }
    match eval_cmp_scalar_inner(entity_val, op, scalar) {
        Some(result) => result,
        None => matches!(op, CmpOp::Unequal),
    }
}

/// Compare one entity value against one scalar, or `None` when the two are not
/// comparable in the scalar's domain.
fn eval_cmp_scalar_inner(entity_val: &Value, op: CmpOp, scalar: &Scalar) -> Option<bool> {
    match scalar {
        Scalar::Number(rhs_num) => {
            let lhs = entity_val_to_f64(entity_val)?;
            let rhs = rhs_num.as_f64()?;
            Some(cmp_op(lhs.partial_cmp(&rhs)?, op))
        }
        Scalar::Bool(rhs_bool) => {
            let lhs = entity_val_to_bool(entity_val)?;
            Some(cmp_op(lhs.cmp(rhs_bool), op))
        }
        Scalar::Str(rhs_str) => {
            let lhs = entity_value_to_string(entity_val)?;
            Some(cmp_op(lhs.cmp(rhs_str), op))
        }
        // A stored temporal value is compared in its own domain, not as text:
        // clause 4.6.3 lets the same instant be written with any offset, and
        // lexicographic order over those renderings is not chronological order.
        // An entity value that is not a temporal literal of the operand's kind
        // is not comparable with it, so the predicate does not hold.
        Scalar::DateTime(rhs) => {
            let lhs = parse_date_time(&entity_value_to_string(entity_val)?).ok()?;
            Some(cmp_op(lhs.cmp(rhs), op))
        }
        Scalar::Date(rhs) => {
            let lhs = parse_date(&entity_value_to_string(entity_val)?).ok()?;
            Some(cmp_op(lhs.cmp(rhs), op))
        }
        Scalar::Time(rhs) => {
            let lhs = parse_time(&entity_value_to_string(entity_val)?).ok()?;
            Some(cmp_op(lhs.cmp(rhs), op))
        }
    }
}

/// Apply a comparison operator to an ordering.
const fn cmp_op(ord: Ordering, op: CmpOp) -> bool {
    match op {
        CmpOp::Equal => ord.is_eq(),
        CmpOp::Unequal => !ord.is_eq(),
        CmpOp::Greater => ord.is_gt(),
        CmpOp::GreaterEq => ord.is_ge(),
        CmpOp::Less => ord.is_lt(),
        CmpOp::LessEq => ord.is_le(),
    }
}

/// Read an entity value as a number, accepting the textual form PostgreSQL and
/// JSON-LD both admit.
fn entity_val_to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Read an entity value as a boolean, accepting its textual form.
fn entity_val_to_bool(val: &Value) -> Option<bool> {
    match val {
        Value::Bool(b) => Some(*b),
        // `&str` is unbounded, so a catch-all is the only way to close this match.
        Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match; anything else is not a boolean in any spelling.
            _ => None,
        },
        Value::Null | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Render an entity value as text for string, pattern and temporal comparisons.
fn entity_value_to_string(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()), // clone: the comparison chain owns its operand
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parser::parse;

    fn vehicle_entity() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]
            }],
            "https://example.com/temperature": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 23.5}]
            }],
            "https://example.com/brandName": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Mercedes"}]
            }],
            "https://example.com/isParked": [{
                "https://uri.etsi.org/ngsi-ld/hasObject": [{
                    "@id": "urn:ngsi-ld:OffStreetParking:Downtown1"
                }]
            }],
            "https://example.com/active": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": true}]
            }],
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@value": {
                        "city": "Berlin",
                        "country": "DE"
                    }
                }]
            }]
        }])
    }

    #[test]
    fn eval_existence_present() {
        let expr = parse("\"https://example.com/speed\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_existence_absent() {
        let expr = parse("\"https://example.com/nonexistent\"").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_greater() {
        let expr = parse("\"https://example.com/speed\">50").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_greater_false() {
        let expr = parse("\"https://example.com/speed\">100").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_equal() {
        let expr = parse("\"https://example.com/speed\"==80").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_unequal() {
        let expr = parse("\"https://example.com/speed\"!=50").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_float_equal() {
        let expr = parse("\"https://example.com/temperature\"==23.5").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_string_equal() {
        let expr = parse("\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_context_source_normalized_property() {
        let properties = serde_json::json!({
            "https://example.com/sourceGroup": {"type": "Property", "value": "alpha"}
        });
        let expr = parse("\"https://example.com/sourceGroup\"==\"alpha\"").unwrap();
        assert!(evaluate(&expr, &properties));
    }

    #[test]
    fn eval_context_source_scalar_property() {
        let properties = serde_json::json!({"https://example.com/sourceGroup": "alpha"});
        let expr = parse("\"https://example.com/sourceGroup\"==\"alpha\"").unwrap();
        assert!(evaluate(&expr, &properties));
    }

    #[test]
    fn eval_string_unequal() {
        let expr = parse("\"https://example.com/brandName\"!=\"BMW\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_relationship_equal() {
        let expr = parse("\"https://example.com/isParked\"==\"urn:ngsi-ld:OffStreetParking:Downtown1\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_boolean() {
        let expr = parse("\"https://example.com/active\"==true").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_range() {
        let expr = parse("\"https://example.com/speed\"==70..90").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_range_outside() {
        let expr = parse("\"https://example.com/speed\"==90..100").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_value_list_match() {
        let expr = parse("\"https://example.com/brandName\"==\"BMW\",\"Mercedes\",\"Audi\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_value_list_no_match() {
        let expr = parse("\"https://example.com/brandName\"==\"BMW\",\"Audi\"").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_match_pattern() {
        let expr = parse("\"https://example.com/brandName\"~=\"Merc.*\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_not_match_pattern() {
        let expr = parse("\"https://example.com/brandName\"!~=\"BMW.*\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_and() {
        let expr = parse("\"https://example.com/speed\">50;\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_and_one_false() {
        let expr = parse("\"https://example.com/speed\">100;\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_or() {
        let expr = parse("\"https://example.com/speed\">100|\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_compound_value_path() {
        let expr = parse("\"https://example.com/address\"[city]==\"Berlin\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_compound_value_path_no_match() {
        let expr = parse("\"https://example.com/address\"[city]==\"Paris\"").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    /// Two `datasetId`-partitioned instances of the same attribute.
    fn multi_instance_entity() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:slow"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 60}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:fast"}]
                }
            ]
        }])
    }

    #[test]
    fn eval_matches_when_any_instance_satisfies_the_predicate() {
        // Clause 4.9 p. 86: the target value is any value of the instances.
        // The first instance is 10, so a first-instance-only evaluator says no.
        let expr = parse("\"https://example.com/speed\">50").unwrap();
        assert!(evaluate(&expr, &multi_instance_entity()));
    }

    #[test]
    fn eval_rejects_when_no_instance_satisfies_the_predicate() {
        let expr = parse("\"https://example.com/speed\">100").unwrap();
        assert!(!evaluate(&expr, &multi_instance_entity()));
    }

    #[test]
    fn eval_not_match_pattern_requires_the_attribute_to_be_present() {
        // Clause 4.9: notPatternOp requires the entity to contain the target
        // element, so a missing attribute does not match.
        let expr = parse("\"https://example.com/missing\"!~=\"x\"").unwrap();
        assert!(!evaluate(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_not_match_pattern_still_matches_a_present_non_matching_value() {
        let expr = parse("\"https://example.com/brandName\"!~=\"BMW.*\"").unwrap();
        assert!(evaluate(&expr, &vehicle_entity()));
    }

    /// An entity exercising the attribute families and the sub-attribute shape a
    /// `q` has to reach beyond Property/Relationship.
    fn rich_entity() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.com/Device"],
            "https://example.com/temperature": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2024-06-01T00:00:00Z"}]
            }],
            "https://example.com/description": [{
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                    {"@language": "en", "@value": "Door"},
                    {"@language": "de", "@value": "Tuer"}
                ]
            }],
            "https://example.com/tags": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": [1, 2, 3]}]
            }],
            "https://example.com/owner": [{
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:42"}]
            }]
        }])
    }

    #[test]
    fn a_temporal_sub_attribute_resolves_to_its_direct_value() {
        // Clause 4.9: `temperature.observedAt` terminates at a bare `@value`
        // node, carrying neither `hasValue` nor `hasObject`; the evaluator must
        // still reach it rather than resolving nothing.
        let expr = parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-01-01T00:00:00Z\"").unwrap();
        assert!(evaluate(&expr, &rich_entity()));
    }

    #[test]
    fn a_language_property_satisfies_a_bare_existence_check() {
        // Clause 4.9: existence holds for any attribute family, including a
        // LanguageProperty, which the hasValue/hasObject-only reader missed.
        let expr = parse("\"https://example.com/description\"").unwrap();
        assert!(evaluate(&expr, &rich_entity()));
    }

    #[test]
    fn values_of_different_types_are_considered_unequal() {
        // Clause 4.9: `!=` holds and `==` fails when the operand's type differs
        // from the target's — here a boolean attribute against a number.
        assert!(evaluate(&parse("\"https://example.com/active\"!=5").unwrap(), &vehicle_entity()));
        assert!(!evaluate(&parse("\"https://example.com/active\"==5").unwrap(), &vehicle_entity()));
    }

    #[test]
    fn an_array_target_is_matched_by_membership() {
        // Clause 4.9: `==` holds when the operand is included in an array value.
        assert!(evaluate(&parse("\"https://example.com/tags\"==2").unwrap(), &rich_entity()));
        assert!(!evaluate(&parse("\"https://example.com/tags\"==9").unwrap(), &rich_entity()));
        assert!(evaluate(&parse("\"https://example.com/tags\"!=9").unwrap(), &rich_entity()));
    }

    #[test]
    fn a_pattern_operator_does_not_match_a_non_string_target() {
        // Clause 4.9: a non-String target is considered not matching, so the
        // number 80 is not stringified to "80" and matched against "8.*".
        assert!(!evaluate(&parse("\"https://example.com/speed\"~=\"8.*\"").unwrap(), &vehicle_entity()));
    }

    #[test]
    fn an_ordering_comparison_against_a_relationship_does_not_match() {
        // Clause 4.9: `>`/`<`/`>=`/`<=` against a Relationship shall not match,
        // but equality against its object still does.
        assert!(!evaluate(&parse("\"https://example.com/owner\">\"a\"").unwrap(), &rich_entity()));
        assert!(evaluate(
            &parse("\"https://example.com/owner\"==\"urn:ngsi-ld:Person:42\"").unwrap(),
            &rich_entity()
        ));
    }

    #[test]
    fn a_target_incomparable_with_a_range_is_outside_it() {
        // "Outside" is the exact complement of "within", so `!=` a numeric range
        // holds for a string target that cannot be inside it.
        assert!(evaluate(&parse("\"https://example.com/brandName\"!=70..90").unwrap(), &vehicle_entity()));
    }

    // -- linked-entity traversal (clause 4.9) --------------------------------

    /// A vehicle whose `isParkedIn` Relationship carries the linked entity
    /// inline, the shape clause 4.5.23.2 `join=inline` produces, expanded.
    fn vehicle_with_inline_parking(category: &str, linked_type: &str) -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/isParkedIn": [{
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:OffStreetParking:Downtown1"}],
                "https://uri.etsi.org/ngsi-ld/entity": [{
                    "@id": "urn:ngsi-ld:OffStreetParking:Downtown1",
                    "@type": [linked_type],
                    "https://example.com/category": [{
                        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]
                    }]
                }]
            }]
        }])
    }

    fn linked_predicate(inner_value: &str) -> Expr {
        parse(&format!(
            "\"https://example.com/isParkedIn\"{{\"https://example.com/category\"==\"{inner_value}\"}}"
        ))
        .expect("parses")
    }

    #[test]
    fn the_embedded_entity_member_is_the_one_the_inline_join_renderer_writes() {
        // The renderer inserts the linked entity under the compacted `entity`
        // key; the core context expands that term to this IRI, which is what an
        // expanded document the evaluator sees carries.
        pretty_assertions::assert_eq!(ENTITY.as_str(), "https://uri.etsi.org/ngsi-ld/entity");
    }

    #[test]
    fn a_hop_matches_on_the_value_of_the_embedded_linked_entity() {
        let entity = vehicle_with_inline_parking("commercial", "https://example.com/Building");
        assert!(evaluate(&linked_predicate("commercial"), &entity));
    }

    #[test]
    fn a_hop_rejects_when_the_embedded_linked_entity_holds_another_value() {
        let entity = vehicle_with_inline_parking("residential", "https://example.com/Building");
        assert!(!evaluate(&linked_predicate("commercial"), &entity));
    }

    #[test]
    fn a_type_guard_admits_only_a_linked_entity_of_a_listed_type() {
        // Clause 4.9's `*1(EntityType 1*("," EntityType) ":")` constrains which
        // linked entities the hop descends into.
        let guarded = parse(concat!(
            "\"https://example.com/isParkedIn\"{\"https://example.com/Building\",",
            "\"https://example.com/Store\":\"https://example.com/category\"==\"commercial\"}"
        ))
        .expect("parses");
        assert!(evaluate(&guarded, &vehicle_with_inline_parking("commercial", "https://example.com/Building")));
        assert!(!evaluate(&guarded, &vehicle_with_inline_parking("commercial", "https://example.com/Warehouse")));
    }

    #[test]
    fn a_hop_without_embedded_linked_data_resolves_no_target() {
        // The evaluator only ever sees the document it is handed. A Relationship
        // that names its target by URI alone carries nothing to descend into, so
        // the predicate does not hold; for a stored entity the SQL compilation
        // answers the traversal instead.
        let entity = serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/isParkedIn": [{
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:OffStreetParking:Downtown1"}]
            }]
        }]);
        assert!(!evaluate(&linked_predicate("commercial"), &entity));
        assert!(!evaluate(
            &parse("\"https://example.com/isParkedIn\"{\"https://example.com/category\"}").expect("parses"),
            &entity
        ));
    }

    #[test]
    fn two_hops_descend_through_two_embedded_levels() {
        let entity = serde_json::json!([{
            "@id": "urn:a",
            "@type": ["https://example.com/A"],
            "https://example.com/outer": [{
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:b"}],
                "https://uri.etsi.org/ngsi-ld/entity": [{
                    "@id": "urn:b",
                    "@type": ["https://example.com/B"],
                    "https://example.com/inner": [{
                        "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:c"}],
                        "https://uri.etsi.org/ngsi-ld/entity": [{
                            "@id": "urn:c",
                            "@type": ["https://example.com/C"],
                            "https://example.com/zone": [{
                                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "north"}]
                            }]
                        }]
                    }]
                }]
            }]
        }]);
        let two_hops = |zone: &str| {
            parse(&format!(
                "\"https://example.com/outer\"{{\"https://example.com/inner\"{{\"https://example.com/zone\"==\"{zone}\"}}}}"
            ))
            .expect("parses")
        };
        assert!(evaluate(&two_hops("north"), &entity));
        assert!(!evaluate(&two_hops("south"), &entity));
    }

    #[test]
    fn the_language_suppression_form_matches_any_language_value() {
        // Clause 4.9: `[*]` matches a LanguageProperty against every language's
        // value, so a match on a non-default language still holds — the default
        // single-value reading would only ever see the first language.
        assert!(evaluate(&parse("\"https://example.com/description\"[*]==\"Tuer\"").unwrap(), &rich_entity()));
        assert!(!evaluate(&parse("\"https://example.com/description\"==\"Tuer\"").unwrap(), &rich_entity()));
    }
}
