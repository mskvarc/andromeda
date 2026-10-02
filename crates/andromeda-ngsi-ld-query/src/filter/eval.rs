//! In-memory evaluation of parsed `q` filter expressions against entities.
//!
//! What a Query Term compares against is resolved elsewhere:
//! [`attribute_axis`](crate::filter::attribute_axis) walks the axis a Query Term
//! names, and [`family_target`](crate::filter::family_target) owns the clause
//! 4.9 definition of the target value each attribute family carries. This module
//! owns the operators clause 4.9 applies to that target.

use crate::{
    filter::{
        attribute_axis::{addresses_element, resolve_targets},
        expression::Expr,
        path::QueryPath,
        predicate::{CmpOp, Predicate, SetMembership},
        scalar::{CompareValue, Scalar, parse_date, parse_date_time, parse_time},
        target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue},
    },
    queried_entity::attribute_reading::QueriedEntity,
};
use std::cmp::Ordering;

/// Evaluate a parsed `q` expression (clause 4.9) against an entity.
///
/// The expression's attribute names are expected expanded, as the entity keys
/// its attributes by expanded name.
pub fn evaluate<'a, E: QueriedEntity<'a>>(expr: &Expr, entity: E) -> bool {
    expression_holds(expr, &|path| resolve_targets(entity, path), &|path| addresses_element(entity, path))
}

/// Evaluate the boolean structure of an expression over the targets
/// `targets_of` resolves each Query Term's attribute axis to, with
/// `element_exists` answering a bare attribute path.
pub(crate) fn expression_holds<'t, F, X>(expr: &Expr, targets_of: &F, element_exists: &X) -> bool
where
    F: Fn(&QueryPath) -> Vec<ResolvedTarget<'t>>,
    X: Fn(&QueryPath) -> bool,
{
    match expr {
        Expr::And(terms) => terms.iter().all(|term| expression_holds(term, targets_of, element_exists)),
        Expr::Or(terms) => terms.iter().any(|term| expression_holds(term, targets_of, element_exists)),
        Expr::Pred(pred) => predicate_holds(pred, targets_of, element_exists),
    }
}

/// Evaluate one leaf predicate.
fn predicate_holds<'t, F, X>(pred: &Predicate, targets_of: &F, element_exists: &X) -> bool
where
    F: Fn(&QueryPath) -> Vec<ResolvedTarget<'t>>,
    X: Fn(&QueryPath) -> bool,
{
    match pred {
        // Clause 4.9: existence holds when the entity "define[s] the target
        // element", whatever attribute family it belongs to and whatever value
        // it holds.
        Predicate::Exists(path) => element_exists(path),
        Predicate::Equality { path, membership, rhs } => targets_of(path).iter().any(|target| eval_equality(target, *membership, rhs)),
        Predicate::Ordering { path, op, rhs } => targets_of(path).iter().any(|target| {
            // Clause 4.9: an ordering comparison against a Relationship shall
            // not match — a URI has no `<`/`>` order the query can mean.
            if is_relationship(target) {
                return false;
            }
            any_one_value(target).is_some_and(|values| values.iter().any(|value| compare(value, op.as_cmp_op(), rhs)))
                || compare(&target.value, op.as_cmp_op(), rhs)
        }),
        // Clause 4.9: a pattern operator applies only to a String target; a
        // non-String value is considered as not matching, so a number or
        // boolean is never stringified into a match. A Relationship's object is
        // excluded for the same reason the ordering operators exclude it — the
        // standard requires a Relationship operand to non-match every operator
        // other than equality and inequality — and a URI is string-shaped, so
        // only its origin can rule it out.
        Predicate::MatchPattern { path, pattern } => targets_of(path)
            .iter()
            .any(|target| pattern_targets(target).iter().any(|value| pattern.is_match(value))),
        // `notPatternOp` also requires the entity to *contain* the target
        // element (clause 4.9); an entity that lacks the attribute, or carries
        // only non-String values, does not match. Where the target is "a value
        // from any of the key-value pairs" of a languageMap the clause reads the
        // negated operator as its complement — no value may match — rather than
        // as one value that happens not to.
        Predicate::NotMatchPattern { path, pattern } => targets_of(path).iter().any(|target| {
            let texts = pattern_targets(target);
            !texts.is_empty() && !texts.iter().any(|value| pattern.is_match(value))
        }),
    }
}

/// Whether a resolved target is a Relationship's object, which clause 4.9 bars
/// from every operator other than equality and inequality.
const fn is_relationship(target: &ResolvedTarget<'_>) -> bool {
    matches!(target.origin, TargetOrigin::RelationshipObject)
}

/// The texts a pattern operator may be applied to, empty when the target offers
/// none (clause 4.9).
///
/// One text for an ordinary target, and one per value where clause 4.9 makes any
/// one of them the target value. An array that *is* the target value offers
/// none: "If the target value data type is different than String then it shall
/// be considered as not matching."
fn pattern_targets<'t>(target: &'t ResolvedTarget<'_>) -> Vec<&'t str> {
    if is_relationship(target) {
        return Vec::new();
    }
    match any_one_value(target) {
        Some(values) => values.iter().filter_map(TargetValue::as_text).collect(),
        None => target.value.as_text().into_iter().collect(),
    }
}

/// The values a target holds when clause 4.9 makes any one of them the target
/// value, rather than the array of them.
fn any_one_value<'t, 'a>(target: &'t ResolvedTarget<'a>) -> Option<&'t [TargetValue<'a>]> {
    match (target.reading, &target.value) {
        (MultiValueReading::AnyOneValue, TargetValue::Array(values)) => Some(values),
        (MultiValueReading::AnyOneValue, TargetValue::Number(_) | TargetValue::Text(_) | TargetValue::Boolean(_) | TargetValue::Json(_))
        | (MultiValueReading::WholeArray, _) => None,
    }
}

/// Evaluate an equality comparison (`==`/`!=`) against a scalar, a range, or a
/// value list.
///
/// "Outside" is the exact complement of "within" — for a range as for a value
/// list — so a target incomparable with the bounds is outside the range, not
/// neither in nor out. The exception is a Range target that is a Relationship
/// object: clause 4.9 forbids the ordering operators against a Relationship in
/// either direction, and a range is two ordering comparisons, so neither
/// membership polarity holds — "outside" is not a match here either, mirroring
/// the Ordering predicate arm above and the SQL compiler (`projection.rs`),
/// which routes `<`/`>`/`<=`/`>=` away from `value_uri` for both range halves.
fn eval_equality(target: &ResolvedTarget<'_>, membership: SetMembership, rhs: &CompareValue) -> bool {
    match rhs {
        CompareValue::Scalar(scalar) => compare(&target.value, membership.as_cmp_op(), scalar),
        CompareValue::Range { min, max } => {
            if is_relationship(target) {
                return false;
            }
            let within = compare(&target.value, CmpOp::GreaterEq, min) && compare(&target.value, CmpOp::LessEq, max);
            match membership {
                SetMembership::Within => within,
                SetMembership::Outside => !within,
            }
        }
        CompareValue::ValueList(values) => {
            let any_match = values.iter().any(|value| compare(&target.value, CmpOp::Equal, value));
            match membership {
                SetMembership::Within => any_match,
                SetMembership::Outside => !any_match,
            }
        }
    }
}

/// Compare one target value against one scalar, treating an unusable pairing
/// as a non-match.
///
/// An array target is compared by membership (clause 4.9): `==` holds when the
/// operand is one of its elements, `!=` when it is not, and an ordering
/// comparison against an array does not hold. Two values of different types are
/// considered unequal, so an incomparable pairing satisfies `!=` and fails `==`
/// and every ordering comparison.
fn compare(value: &TargetValue<'_>, op: CmpOp, scalar: &Scalar) -> bool {
    if let TargetValue::Array(elements) = value {
        let contains = elements.iter().any(|element| compare(element, CmpOp::Equal, scalar));
        return match op {
            CmpOp::Equal => contains,
            CmpOp::Unequal => !contains,
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => false,
        };
    }
    match compare_in_domain(value, op, scalar) {
        Some(result) => result,
        None => matches!(op, CmpOp::Unequal),
    }
}

/// Compare one target value against one scalar, or `None` when the two are not
/// comparable in the scalar's domain.
fn compare_in_domain(value: &TargetValue<'_>, op: CmpOp, scalar: &Scalar) -> Option<bool> {
    match scalar {
        Scalar::Number(rhs) => Some(cmp_op(value.as_number()?.as_ref().cmp(rhs), op)),
        Scalar::Bool(rhs) => Some(cmp_op(value.as_boolean()?.cmp(rhs), op)),
        Scalar::Str(rhs) => Some(cmp_op(value.as_lexical()?.as_ref().cmp(rhs.as_str()), op)),
        // A stored temporal value is compared in its own domain, not as text:
        // clause 4.6.3 lets the same instant be written with any offset, and
        // lexicographic order over those renderings is not chronological order.
        // A target value that is not a temporal literal of the operand's kind
        // is not comparable with it, so the predicate does not hold.
        Scalar::DateTime(rhs) => Some(cmp_op(parse_date_time(&value.as_lexical()?).ok()?.cmp(rhs), op)),
        Scalar::Date(rhs) => Some(cmp_op(parse_date(&value.as_lexical()?).ok()?.cmp(rhs), op)),
        Scalar::Time(rhs) => Some(cmp_op(parse_time(&value.as_lexical()?).ok()?.cmp(rhs), op)),
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

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{eval::evaluate, expansion::expand_q_expression, expression::Expr, json_key_attributes::JsonKeyAttributes, parser::parse},
        queried_entity::expanded_fixture::FixtureEntity,
        term_expander::TermExpander,
    };
    use iri_rs::IriBuf;
    use serde_json::Value;
    use std::future::{Future, ready};

    /// Whether an expanded entity satisfies an expression, read through the
    /// entity traits.
    fn holds(expr: &Expr, document: &Value) -> bool {
        evaluate(expr, &FixtureEntity::from_expanded(document))
    }

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
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_existence_absent() {
        let expr = parse("\"https://example.com/nonexistent\"").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_greater() {
        let expr = parse("\"https://example.com/speed\">50").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_greater_false() {
        let expr = parse("\"https://example.com/speed\">100").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_equal() {
        let expr = parse("\"https://example.com/speed\"==80").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_number_unequal() {
        let expr = parse("\"https://example.com/speed\"!=50").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_float_equal() {
        let expr = parse("\"https://example.com/temperature\"==23.5").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn exact_decimals_compare_beyond_binary64_precision() {
        let entity: Value = serde_json::from_str(
            r#"[{"@id":"urn:ngsi-ld:Sensor:1","https://example.com/reading":[{"https://uri.etsi.org/ngsi-ld/hasValue":[{"@value":0.100000000000000005}]}]}]"#,
        )
        .expect("exact arbitrary-precision JSON");
        assert!(holds(
            &parse("\"https://example.com/reading\"==0.100000000000000005").expect("exact equality"),
            &entity
        ));
        assert!(holds(
            &parse("\"https://example.com/reading\">0.100000000000000004").expect("exact ordering"),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/reading\"==0.100000000000000004").expect("adjacent inequality"),
            &entity
        ));
    }

    #[test]
    fn a_typed_json_ld_numeric_string_uses_the_same_exact_domain() {
        let entity = serde_json::json!([{
            "@id": "urn:ngsi-ld:Sensor:1",
            "https://example.com/reading": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "0.100000000000000005", "@type": "http://www.w3.org/2001/XMLSchema#decimal"}]
            }]
        }]);
        assert!(holds(
            &parse("\"https://example.com/reading\"==0.100000000000000005").expect("exact equality"),
            &entity
        ));
    }

    #[test]
    fn eval_string_equal() {
        let expr = parse("\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_string_unequal() {
        let expr = parse("\"https://example.com/brandName\"!=\"BMW\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_relationship_equal() {
        let expr = parse("\"https://example.com/isParked\"==\"urn:ngsi-ld:OffStreetParking:Downtown1\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_boolean() {
        let expr = parse("\"https://example.com/active\"==true").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_range() {
        let expr = parse("\"https://example.com/speed\"==70..90").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_range_outside() {
        let expr = parse("\"https://example.com/speed\"==90..100").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_value_list_match() {
        let expr = parse("\"https://example.com/brandName\"==\"BMW\",\"Mercedes\",\"Audi\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_value_list_no_match() {
        let expr = parse("\"https://example.com/brandName\"==\"BMW\",\"Audi\"").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_match_pattern() {
        let expr = parse("\"https://example.com/brandName\"~=\"Merc.*\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_not_match_pattern() {
        let expr = parse("\"https://example.com/brandName\"!~=\"BMW.*\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_and() {
        let expr = parse("\"https://example.com/speed\">50;\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_and_one_false() {
        let expr = parse("\"https://example.com/speed\">100;\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_or() {
        let expr = parse("\"https://example.com/speed\">100|\"https://example.com/brandName\"==\"Mercedes\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_compound_value_path() {
        let expr = parse("\"https://example.com/address\"[city]==\"Berlin\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_compound_value_path_no_match() {
        let expr = parse("\"https://example.com/address\"[city]==\"Paris\"").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    /// A structured Property value expanded by JSON-LD, which is the shape a
    /// notification payload and a stored entity carry: the members are IRI-keyed
    /// nodes hanging off `hasValue`, not keys inside one `@value`. An evaluator
    /// that read the terminal `@value` first and then indexed into it resolved
    /// nothing here, so a subscription filtering on a compound member never
    /// matched.
    fn entity_with_expanded_compound_value() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://example.com/Building"],
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/city": [{"@value": "Berlin"}],
                    "https://example.com/country": [{"@value": "DE"}]
                }]
            }],
            "https://example.com/sensor": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/airquality": [{
                        "https://example.com/particulate": [{"@value": 40}]
                    }]
                }]
            }]
        }])
    }

    #[test]
    fn a_compound_member_of_an_expanded_value_is_reached_through_its_member_node() {
        let entity = entity_with_expanded_compound_value();
        assert!(holds(
            &parse("\"https://example.com/address\"[\"https://example.com/city\"]==\"Berlin\"").unwrap(),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/address\"[\"https://example.com/city\"]==\"Paris\"").unwrap(),
            &entity
        ));
    }

    #[test]
    fn a_nested_compound_member_is_reached_through_one_hop_per_member() {
        let entity = entity_with_expanded_compound_value();
        assert!(holds(
            &parse("\"https://example.com/sensor\"[\"https://example.com/airquality\".\"https://example.com/particulate\"]==40").unwrap(),
            &entity
        ));
        assert!(holds(
            &parse("\"https://example.com/sensor\"[\"https://example.com/airquality\".\"https://example.com/particulate\"]>30").unwrap(),
            &entity
        ));
    }

    #[test]
    fn a_compound_member_the_value_does_not_carry_resolves_no_target() {
        let entity = entity_with_expanded_compound_value();
        assert!(!holds(
            &parse("\"https://example.com/address\"[\"https://example.com/postcode\"]==\"10115\"").unwrap(),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/address\"[\"https://example.com/postcode\"]").unwrap(),
            &entity
        ));
    }

    /// An expander that prefixes every term, standing in for a request
    /// `@context` without needing a JSON-LD engine.
    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    /// A `JsonProperty`, whose value is raw JSON that JSON-LD expansion never
    /// touches: `hasJSON`'s `@value` holds the object exactly as the client
    /// wrote it, keys included.
    fn entity_with_json_property() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://example.com/Building"],
            "https://example.com/myJson": [{
                "https://uri.etsi.org/ngsi-ld/hasJSON": [{
                    "@value": {"city": "Berlin"}
                }]
            }]
        }])
    }

    /// The defect a term-expanded `value_path` reintroduced: `resolve_member_hops`
    /// cannot walk into a `JsonProperty`'s raw `@value` object (it carries no
    /// IRI-keyed member nodes to hop through), so a compound `q` over one has to
    /// fall back to the raw-JSON walk — and that walk has to use the key the
    /// client wrote, not the IRI the path was expanded to. `eval_compound_value_path`
    /// above parses without ever calling `expand_q_expression`, so it cannot
    /// exercise this: `path.value_path` there is already the compact spelling.
    #[tokio::test]
    async fn a_json_property_compound_member_is_still_reached_after_the_path_is_expanded() {
        let expanded = expand_q_expression(Some("myJson[city]==\"Berlin\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        assert!(holds(&expanded, &entity_with_json_property()));

        let non_matching = expand_q_expression(Some("myJson[city]==\"Paris\""), &JsonKeyAttributes::default(), &PrefixExpander)
            .await
            .expect("expands")
            .expect("present");
        assert!(!holds(&non_matching, &entity_with_json_property()));
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
        assert!(holds(&expr, &multi_instance_entity()));
    }

    #[test]
    fn eval_rejects_when_no_instance_satisfies_the_predicate() {
        let expr = parse("\"https://example.com/speed\">100").unwrap();
        assert!(!holds(&expr, &multi_instance_entity()));
    }

    #[test]
    fn eval_not_match_pattern_requires_the_attribute_to_be_present() {
        // Clause 4.9: notPatternOp requires the entity to contain the target
        // element, so a missing attribute does not match.
        let expr = parse("\"https://example.com/missing\"!~=\"x\"").unwrap();
        assert!(!holds(&expr, &vehicle_entity()));
    }

    #[test]
    fn eval_not_match_pattern_still_matches_a_present_non_matching_value() {
        let expr = parse("\"https://example.com/brandName\"!~=\"BMW.*\"").unwrap();
        assert!(holds(&expr, &vehicle_entity()));
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
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}, {"@value": 2}, {"@value": 3}]
            }],
            "https://example.com/colors": [{
                "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}, {"@value": "blue"}]}]
            }],
            "https://example.com/parts": [{
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [{"@value": "urn:ngsi-ld:Part:1"}, {"@value": "urn:ngsi-ld:Part:2"}]}]
            }],
            "https://example.com/gender": [{
                "https://uri.etsi.org/ngsi-ld/hasVocab": [{"@id": "https://example.com/Male"}, {"@id": "https://example.com/Other"}]
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
        assert!(holds(&expr, &rich_entity()));
    }

    #[test]
    fn a_language_property_satisfies_a_bare_existence_check() {
        // Clause 4.9: existence holds for any attribute family, including a
        // LanguageProperty, which the hasValue/hasObject-only reader missed.
        let expr = parse("\"https://example.com/description\"").unwrap();
        assert!(holds(&expr, &rich_entity()));
    }

    /// Clause 4.9: "When a Query Term only defines an attribute path ..., the
    /// matching Entities shall be those which define the target element
    /// (Property or a Relationship), regardless of any target value or
    /// object." A structured Property value and a `GeoProperty` held as a
    /// geometry carry no literal, yet each defines its target element. The JSON
    /// document reading answers this by a comparable value and so disagrees,
    /// which is why these assertions read the entity traits alone.
    #[test]
    fn a_bare_attribute_path_holds_for_a_structured_value_and_a_geometry() {
        let entity = serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://example.com/Building"],
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/city": [{"@value": "Berlin"}],
                    "https://example.com/geo": [{"https://example.com/lat": [{"@value": 52}]}]
                }]
            }],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.4}, {"@value": 52.5}]}]
                }]
            }]
        }]);
        let fixture = FixtureEntity::from_expanded(&entity);
        let exists = |q: &str| evaluate(&parse(q).expect("parses"), &fixture);
        assert!(exists("\"https://example.com/address\""), "a structured Property value defines its element");
        assert!(exists("\"https://uri.etsi.org/ngsi-ld/location\""), "a geometry defines its element");
        assert!(
            exists("\"https://example.com/address\"[\"https://example.com/geo\"]"),
            "a member holding a structured value is a defined value"
        );
        assert!(!exists("\"https://example.com/address\"[\"https://example.com/postcode\"]"));
        assert!(!exists("\"https://example.com/missing\""));
        assert!(!exists("\"https://example.com/address\".\"https://uri.etsi.org/ngsi-ld/observedAt\""));
    }

    #[test]
    fn values_of_different_types_are_considered_unequal() {
        // Clause 4.9: `!=` holds and `==` fails when the operand's type differs
        // from the target's — here a boolean attribute against a number.
        assert!(holds(&parse("\"https://example.com/active\"!=5").unwrap(), &vehicle_entity()));
        assert!(!holds(&parse("\"https://example.com/active\"==5").unwrap(), &vehicle_entity()));
    }

    #[test]
    fn an_array_valued_property_is_matched_by_membership() {
        // Clause 4.9, `equal`: the Query Term value "is included in the target
        // value, if the latter is an array"; `unequal`: it "is not included in
        // the target value, if the latter is an array". The array a client wrote
        // as a Property value expands to one `hasValue` node per element, so
        // every element is part of the target value — a reading that stopped at
        // the first node answered `==` for element 1 alone and answered `!=`
        // with the opposite of the clause for every other element.
        assert!(holds(&parse("\"https://example.com/tags\"==1").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/tags\"==3").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/tags\"==9").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/tags\"!=9").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/tags\"!=3").unwrap(), &rich_entity()));
    }

    /// Clause 4.9: "If the target element is a `ListProperty`, the target value
    /// is defined as the valueList array associated to such a `ListProperty`",
    /// and `equal` holds when the operand "is included in the target value, if
    /// the latter is an array".
    #[test]
    fn a_list_property_is_matched_by_membership_of_its_value_list() {
        assert!(holds(&parse("\"https://example.com/colors\"==\"red\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/colors\"==\"blue\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/colors\"==\"green\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/colors\"!=\"green\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/colors\"!=\"blue\"").unwrap(), &rich_entity()));
    }

    /// Clause 4.9: "If the target element is a `ListRelationship`, the target
    /// object is defined as the array of objects associated (represented as
    /// URIs) to such `ListRelationship`", and a URI Query Term matches when it
    /// "is included in the target value, if the latter is an array".
    #[test]
    fn a_list_relationship_is_matched_by_membership_of_its_object_list() {
        assert!(holds(&parse("\"https://example.com/parts\"==\"urn:ngsi-ld:Part:2\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/parts\"==\"urn:ngsi-ld:Part:9\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/parts\"!=\"urn:ngsi-ld:Part:9\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/parts\"!=\"urn:ngsi-ld:Part:1\"").unwrap(), &rich_entity()));
    }

    /// Clause 4.9 bars a `ListRelationship` from every operator other than
    /// equality and inequality, exactly as it bars a Relationship: "the
    /// combination of such target element with any operator different than equal
    /// or unequal shall result in not matching".
    #[test]
    fn a_list_relationship_matches_no_ordering_or_pattern_operator() {
        assert!(!holds(&parse("\"https://example.com/parts\">\"urn:ngsi-ld:Part:0\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/parts\"~=\"urn:.*\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/parts\"!~=\"nothing.*\"").unwrap(), &rich_entity()));
    }

    /// Clause 4.9: "If the target element is a `VocabProperty`, the target value
    /// shall be expanded according to the @context", and it is listed beside a
    /// Relationship as a target a URI Query Term matches when included in it —
    /// so a `VocabProperty` carrying several terms is matched on any of them,
    /// not only the first.
    #[test]
    fn a_multi_term_vocab_property_is_matched_by_membership() {
        assert!(holds(
            &parse("\"https://example.com/gender\"==\"https://example.com/Other\"").unwrap(),
            &rich_entity()
        ));
        assert!(holds(
            &parse("\"https://example.com/gender\"==\"https://example.com/Male\"").unwrap(),
            &rich_entity()
        ));
        assert!(!holds(
            &parse("\"https://example.com/gender\"==\"https://example.com/Female\"").unwrap(),
            &rich_entity()
        ));
        assert!(holds(
            &parse("\"https://example.com/gender\"!=\"https://example.com/Female\"").unwrap(),
            &rich_entity()
        ));
        assert!(!holds(
            &parse("\"https://example.com/gender\"!=\"https://example.com/Male\"").unwrap(),
            &rich_entity()
        ));
    }

    #[test]
    fn a_pattern_operator_does_not_match_a_non_string_target() {
        // Clause 4.9: a non-String target is considered not matching, so the
        // number 80 is not stringified to "80" and matched against "8.*".
        assert!(!holds(&parse("\"https://example.com/speed\"~=\"8.*\"").unwrap(), &vehicle_entity()));
    }

    #[test]
    fn a_pattern_operator_does_not_match_a_relationship_object() {
        // Clause 4.9 requires a Relationship operand to non-match every operator
        // other than equality and inequality. A URI is string-shaped, so only its
        // origin rules it out — and `notPatternOp` must not turn the exclusion
        // into a match either.
        assert!(!holds(&parse("\"https://example.com/owner\"~=\"urn:.*\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/owner\"!~=\"^nothing\"").unwrap(), &rich_entity()));
    }

    #[test]
    fn an_ordering_comparison_against_a_relationship_does_not_match() {
        // Clause 4.9: `>`/`<`/`>=`/`<=` against a Relationship shall not match,
        // but equality against its object still does.
        assert!(!holds(&parse("\"https://example.com/owner\">\"a\"").unwrap(), &rich_entity()));
        assert!(holds(
            &parse("\"https://example.com/owner\"==\"urn:ngsi-ld:Person:42\"").unwrap(),
            &rich_entity()
        ));
    }

    #[test]
    fn a_range_comparison_against_a_relationship_does_not_match_in_either_polarity() {
        // Clause 4.9: a range is two ordering comparisons, and ordering shall
        // not match a Relationship object in either direction — so neither
        // `==` (within) nor `!=` (outside) holds, mirroring `projection.rs`'s
        // `a_string_range_bound_keeps_relationship_objects_out_of_its_ordering_halves`.
        assert!(!holds(&parse("\"https://example.com/owner\"==\"urn:a\"..\"urn:z\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/owner\"!=\"urn:a\"..\"urn:z\"").unwrap(), &rich_entity()));
    }

    #[test]
    fn a_target_incomparable_with_a_range_is_outside_it() {
        // "Outside" is the exact complement of "within", so `!=` a numeric range
        // holds for a string target that cannot be inside it.
        assert!(holds(&parse("\"https://example.com/brandName\"!=70..90").unwrap(), &vehicle_entity()));
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
    fn a_hop_matches_on_the_value_of_the_embedded_linked_entity() {
        let entity = vehicle_with_inline_parking("commercial", "https://example.com/Building");
        assert!(holds(&linked_predicate("commercial"), &entity));
    }

    #[test]
    fn a_hop_rejects_when_the_embedded_linked_entity_holds_another_value() {
        let entity = vehicle_with_inline_parking("residential", "https://example.com/Building");
        assert!(!holds(&linked_predicate("commercial"), &entity));
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
        assert!(holds(&guarded, &vehicle_with_inline_parking("commercial", "https://example.com/Building")));
        assert!(!holds(&guarded, &vehicle_with_inline_parking("commercial", "https://example.com/Warehouse")));
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
        assert!(!holds(&linked_predicate("commercial"), &entity));
        assert!(!holds(
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
        assert!(holds(&two_hops("north"), &entity));
        assert!(!holds(&two_hops("south"), &entity));
    }

    /// Clause 4.9's `patternOp` and `notPatternOp` are defined over a String
    /// target, and with no target language a `LanguageProperty`'s target value
    /// is "a value from any of the key-value pairs held within the languageMap"
    /// — a String — so every language's value is a candidate, not only the one
    /// that happens to serialise first. `notPatternOp` is the complement: no
    /// value may match.
    #[test]
    fn a_pattern_operator_reaches_every_language_of_a_language_property() {
        assert!(holds(&parse("\"https://example.com/description\"~=\"Tue.*\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/description\"~=\"Do.*\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/description\"~=\"rot.*\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/description\"!~=\"rot.*\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/description\"!~=\"Tue.*\"").unwrap(), &rich_entity()));
    }

    /// Clause 4.9 defines a `ListProperty`'s target value as the `valueList`
    /// array itself, and a pattern operator over a target whose "data type is
    /// different than String … shall be considered as not matching" — so a
    /// pattern never reaches the elements of a list.
    #[test]
    fn a_pattern_operator_does_not_reach_the_elements_of_a_value_list() {
        assert!(!holds(&parse("\"https://example.com/colors\"~=\"re.*\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/colors\"!~=\"nothing.*\"").unwrap(), &rich_entity()));
    }

    #[test]
    fn a_language_property_with_no_target_language_matches_any_language_value() {
        // Clause 4.9: "If the target element is a LanguageProperty, and no
        // target language is specified, the target value is defined as a value
        // from any of the key-value pairs held within the languageMap" — which
        // is what both spellings of "no target language" address, the bare path
        // and the `[*]` suppression marker. A reading that saw only the first
        // language answered `==` for it alone.
        assert!(holds(&parse("\"https://example.com/description\"[*]==\"Tuer\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/description\"==\"Tuer\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/description\"==\"Door\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/description\"==\"rot\"").unwrap(), &rich_entity()));
        // Clause 4.9, `unequal`: "No matching value is found in any of the
        // values of the key-value pairs of a languageMap", so a map that does
        // hold the value is not a match in either spelling.
        assert!(!holds(&parse("\"https://example.com/description\"!=\"Tuer\"").unwrap(), &rich_entity()));
        assert!(!holds(&parse("\"https://example.com/description\"[*]!=\"Tuer\"").unwrap(), &rich_entity()));
        assert!(holds(&parse("\"https://example.com/description\"!=\"rot\"").unwrap(), &rich_entity()));
    }

    /// An entity whose instances carry the members a `.` segment can name
    /// besides a sub-attribute.
    fn instance_members_entity() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Sensor:1",
            "@type": ["https://example.com/Sensor"],
            "https://example.com/temperature": [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}],
                    "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}],
                    "https://uri.etsi.org/ngsi-ld/createdAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 35}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:b"}]
                }
            ],
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/city": [{"@value": "Berlin"}],
                    "https://example.com/extra": [{"@value": {"floor": 3}, "@type": "@json"}]
                }]
            }]
        }])
    }

    /// Clause 4.9 addresses `datasetId` with `.` like a sub-attribute, and
    /// every instance answers for its own.
    #[test]
    fn the_dataset_of_every_instance_is_addressable() {
        let entity = instance_members_entity();
        let dataset = |id: &str| {
            parse(&format!(
                "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/datasetId\"==\"{id}\""
            ))
            .expect("parses")
        };
        assert!(holds(&dataset("urn:ngsi-ld:Dataset:a"), &entity));
        assert!(holds(&dataset("urn:ngsi-ld:Dataset:b"), &entity));
        assert!(!holds(&dataset("urn:ngsi-ld:Dataset:c"), &entity));
    }

    /// A unit code and a system time are compared like any other String, the
    /// time in its temporal domain when the operand is a `DateTime`.
    #[test]
    fn a_unit_code_and_a_creation_time_are_compared_as_their_lexical_forms() {
        let entity = instance_members_entity();
        assert!(holds(
            &parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/unitCode\"==\"CEL\"").expect("parses"),
            &entity
        ));
        assert!(holds(
            &parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/createdAt\">2024-06-01T09:00:00Z").expect("parses"),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/createdAt\">2024-06-01T11:00:00Z").expect("parses"),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/modifiedAt\"").expect("parses"),
            &entity
        ));
    }

    /// A segment naming the value member itself lands on the first value of
    /// each instance.
    #[test]
    fn a_segment_naming_the_value_member_compares_the_first_value() {
        let entity = instance_members_entity();
        let value_above = |bound: u32| {
            parse(&format!(
                "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/hasValue\">{bound}"
            ))
            .expect("parses")
        };
        assert!(holds(&value_above(30), &entity));
        assert!(!holds(&value_above(40), &entity));
    }

    /// A number compared with a String operand is compared in its JSON
    /// spelling.
    #[test]
    fn a_number_is_equal_to_a_string_spelling_it() {
        assert!(holds(&parse("\"https://example.com/speed\"==\"80\"").expect("parses"), &vehicle_entity()));
        assert!(!holds(&parse("\"https://example.com/speed\"==\"81\"").expect("parses"), &vehicle_entity()));
    }

    /// A member of a structured value reached through the value member, and a
    /// raw JSON literal held inside a structured value.
    #[test]
    fn a_structured_value_is_walked_through_both_axes() {
        let entity = instance_members_entity();
        assert!(holds(
            &parse("\"https://example.com/address\".\"https://uri.etsi.org/ngsi-ld/hasValue\".\"https://example.com/city\"==\"Berlin\"").expect("parses"),
            &entity
        ));
        assert!(!holds(
            &parse("\"https://example.com/address\"[\"https://example.com/extra\".\"https://example.com/floor\"]==3").expect("parses"),
            &entity
        ));
    }
}
