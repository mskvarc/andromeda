//! In-memory evaluation of parsed `q` filter expressions against expanded JSON-LD entities.

use crate::query::q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar};
use regex::Regex;
use serde_json::Value;

/// Evaluate a parsed `q` expression against an expanded JSON-LD entity.
///
/// The entity should be a single expanded JSON-LD object (not wrapped in an array).
pub fn evaluate(expr: &Expr, entity: &Value) -> bool {
    let obj = match entity {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => return false,
    };
    let Some(obj) = obj else { return false };
    eval_expr(expr, obj)
}

fn eval_expr(expr: &Expr, obj: &serde_json::Map<String, Value>) -> bool {
    match expr {
        Expr::And(terms) => terms.iter().all(|t| eval_expr(t, obj)),
        Expr::Or(terms) => terms.iter().any(|t| eval_expr(t, obj)),
        Expr::Pred(pred) => eval_predicate(pred, obj),
    }
}

fn eval_predicate(pred: &Predicate, obj: &serde_json::Map<String, Value>) -> bool {
    match pred {
        Predicate::Exists(path) => resolve_value(obj, path).is_some(),
        Predicate::Compare { path, op, rhs } => {
            let Some(entity_val) = resolve_value(obj, path) else {
                return false;
            };
            eval_compare(&entity_val, *op, rhs)
        }
        Predicate::MatchPattern { path, pattern } => {
            let Some(entity_val) = resolve_value(obj, path) else {
                return false;
            };
            match (entity_val_to_string(&entity_val), Regex::new(pattern)) {
                (Some(s), Ok(re)) => re.is_match(&s),
                _ => false,
            }
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let Some(entity_val) = resolve_value(obj, path) else {
                return true; // attribute doesn't exist -> not matching pattern is vacuously true
            };
            match (entity_val_to_string(&entity_val), Regex::new(pattern)) {
                (Some(s), Ok(re)) => !re.is_match(&s),
                _ => true,
            }
        }
    }
}

/// Resolve the value of an attribute path from an expanded JSON-LD entity object.
///
/// Navigates: `obj[attr][0]` -> sub-attrs -> `hasValue[0].@value` or `hasObject[0].@id`,
/// then optional value_path for compound values.
fn resolve_value(obj: &serde_json::Map<String, Value>, path: &AttrPath) -> Option<Value> {
    use crate::jsonld::vocab;
    let has_value = vocab::VALUE.as_str();
    let has_object = vocab::OBJECT.as_str();

    // Navigate to the attribute instance: obj[attr][0]
    let attr_instances = obj.get(&path.attr)?;
    let instance = attr_instances.as_array()?.first()?;

    // Navigate sub-attributes (e.g. temperature.observedAt)
    let mut current = instance;
    for sub in &path.sub_attrs {
        current = current.get(sub)?.as_array()?.first()?;
    }

    // Try hasValue first, then hasObject
    let (terminal_value, is_object) = if let Some(hv) = current.get(has_value).and_then(|v| v.as_array()?.first()) {
        (hv, false)
    } else if let Some(ho) = current.get(has_object).and_then(|v| v.as_array()?.first()) {
        (ho, true)
    } else {
        return None;
    };

    // Extract the raw value
    let raw = if is_object {
        terminal_value.get("@id")?.clone()
    } else {
        terminal_value.get("@value")?.clone()
    };

    // Navigate compound value path (e.g. address[city])
    if path.value_path.is_empty() {
        Some(raw)
    } else {
        let mut v = &raw;
        for key in &path.value_path {
            v = v.get(key)?;
        }
        Some(v.clone())
    }
}

fn eval_compare(entity_val: &Value, op: CmpOp, rhs: &CompareValue) -> bool {
    match rhs {
        CompareValue::Scalar(scalar) => eval_cmp_scalar(entity_val, op, scalar),
        CompareValue::Range { min, max } => match op {
            CmpOp::Equal => eval_cmp_scalar(entity_val, CmpOp::GreaterEq, min) && eval_cmp_scalar(entity_val, CmpOp::LessEq, max),
            CmpOp::Unequal => eval_cmp_scalar(entity_val, CmpOp::Less, min) || eval_cmp_scalar(entity_val, CmpOp::Greater, max),
            _ => false,
        },
        CompareValue::ValueList(values) => {
            let any_match = values.iter().any(|v| eval_cmp_scalar(entity_val, CmpOp::Equal, v));
            match op {
                CmpOp::Equal => any_match,
                CmpOp::Unequal => !any_match,
                _ => false,
            }
        }
    }
}

fn eval_cmp_scalar(entity_val: &Value, op: CmpOp, scalar: &Scalar) -> bool {
    eval_cmp_scalar_inner(entity_val, op, scalar).unwrap_or(false)
}

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
            let lhs = entity_val_to_string(entity_val)?;
            Some(cmp_op(lhs.cmp(rhs_str), op))
        }
        Scalar::DateTime(rhs_dt) | Scalar::Date(rhs_dt) | Scalar::Time(rhs_dt) => {
            let lhs = entity_val_to_string(entity_val)?;
            Some(cmp_op(lhs.cmp(rhs_dt), op))
        }
    }
}

fn cmp_op(ord: std::cmp::Ordering, op: CmpOp) -> bool {
    match op {
        CmpOp::Equal => ord.is_eq(),
        CmpOp::Unequal => !ord.is_eq(),
        CmpOp::Greater => ord.is_gt(),
        CmpOp::GreaterEq => ord.is_ge(),
        CmpOp::Less => ord.is_lt(),
        CmpOp::LessEq => ord.is_le(),
    }
}

fn entity_val_to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn entity_val_to_bool(val: &Value) -> Option<bool> {
    match val {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn entity_val_to_string(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::q::parser::parse;

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
}
