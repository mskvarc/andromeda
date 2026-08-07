//! JSONB jsonpath fallback for the `q` predicate shapes the `attribute_current`
//! projection cannot answer.
//!
//! The projection stores one typed value per attribute instance. Sub-attributes
//! (`temperature.observedAt`) and the interior of compound values
//! (`address[city]`) live only inside the payload, so predicates addressing them
//! read the payload directly. This path is unindexed by construction.

use crate::{
    jsonb::{attribute_value_jsonpath, comparison_operand, escape_jsonpath},
    operator::{comparison, pattern_match, present_and},
    q_predicate::target::QFilterTarget,
};
use andromeda_ngsi_ld_query::{
    filter::{
        path::AttrPath,
        predicate::{CmpOp, OrderingOp, PatternPolarity, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBJECT, VALUE};
use sea_query::{Condition, Expr as SeaExpr, Value as SeaValue};

/// Compile an existence predicate that carries a sub-path.
///
/// A sub-path existence check asks whether a nested element is present, which
/// only the payload can answer. An earlier compiler silently dropped the
/// sub-path here and tested the top-level attribute instead.
pub fn fallback_exists(path: &AttrPath, target: &QFilterTarget<'_>) -> Condition {
    let jsonpath = value_extraction_jsonpath(path, VALUE.as_str());
    let sql = format!("jsonb_path_exists({}, '{}')", target.payload_column(), escape_jsonpath(&jsonpath));
    Condition::all().add(SeaExpr::cust(sql))
}

/// The `jsonb_path_query_first` extraction reaching one payload member of this
/// attribute.
fn payload_extraction(path: &AttrPath, terminal_iri: &str, target: &QFilterTarget<'_>) -> String {
    let jsonpath = value_extraction_jsonpath(path, terminal_iri);
    format!("jsonb_path_query_first({}, '{}')", target.payload_column(), escape_jsonpath(&jsonpath))
}

/// Build the SQL for a single-scalar comparison, COALESCing a Property's value
/// with a Relationship's object so a comparison over either matches (clause 4.9).
fn scalar_comparison(extract_value: &str, extract_object: &str, scalar: &Scalar, op: CmpOp) -> (String, String) {
    let (cast, value) = comparison_operand(scalar);
    let sql = format!(
        "(COALESCE(({extract_value} #>> '{{}}'), ({extract_object} #>> '{{}}')){cast} {op} $1{cast})",
        op = comparison(op)
    );
    (sql, value)
}

/// Compile an equality predicate (`==`/`!=`) against the payload.
pub fn fallback_equality(path: &AttrPath, membership: SetMembership, rhs: &CompareValue, target: &QFilterTarget<'_>) -> Condition {
    let extract_value = payload_extraction(path, VALUE.as_str(), target);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let extract_object = payload_extraction(path, OBJECT.as_str(), target);
            let (sql, value) = scalar_comparison(&extract_value, &extract_object, scalar, membership.as_cmp_op());
            Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_value) = comparison_operand(min);
            let (_, max_value) = comparison_operand(max);
            // A missing element makes the extraction NULL, and every comparison
            // against NULL is unknown, so presence is required structurally in
            // both polarities (clause 4.9).
            let sql = match membership {
                SetMembership::Within => format!("(({extract_value} #>> '{{}}')){cast} >= $1{cast} AND (({extract_value} #>> '{{}}')){cast} <= $2{cast}"),
                SetMembership::Outside => format!("(({extract_value} #>> '{{}}')){cast} < $1{cast} OR (({extract_value} #>> '{{}}')){cast} > $2{cast}"),
            };
            Condition::all().add(SeaExpr::cust_with_values(sql, [min_value, max_value]))
        }
        CompareValue::ValueList(values) => {
            let mut arms = Vec::with_capacity(values.len());
            let mut binds = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let (cast, bound) = comparison_operand(value);
                arms.push(format!("(({extract_value} #>> '{{}}')){cast} = ${}{cast}", index + 1));
                binds.push(bound);
            }
            let joined = arms.join(" OR ");
            let sql = match membership {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            };
            Condition::all().add(SeaExpr::cust_with_values(sql, binds))
        }
    }
}

/// Compile an ordering predicate (`>`/`>=`/`<`/`<=`) against the payload.
pub fn fallback_ordering(path: &AttrPath, op: OrderingOp, rhs: &Scalar, target: &QFilterTarget<'_>) -> Condition {
    let extract_value = payload_extraction(path, VALUE.as_str(), target);
    let extract_object = payload_extraction(path, OBJECT.as_str(), target);
    let (sql, value) = scalar_comparison(&extract_value, &extract_object, rhs, op.as_cmp_op());
    Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
}

/// Compile a pattern predicate against the payload.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend.
pub fn fallback_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, target: &QFilterTarget<'_>) -> Condition {
    let payload = target.payload_column();
    let value_path = escape_jsonpath(&value_extraction_jsonpath(path, VALUE.as_str()));
    let object_path = escape_jsonpath(&value_extraction_jsonpath(path, OBJECT.as_str()));
    // The target is a Property's value (`hasValue`) or a Relationship's object
    // (`hasObject`); COALESCE both so a `~=`/`!~=` over a Relationship's object
    // URI matches, mirroring `scalar_comparison` (clause 4.9).
    let extract =
        format!("COALESCE((jsonb_path_query_first({payload}, '{value_path}')#>>'{{}}'), (jsonb_path_query_first({payload}, '{object_path}')#>>'{{}}'))");
    let value_sql = format!("({extract})::text {} $1", pattern_match(polarity));
    let present = format!("(jsonb_path_exists({payload}, '{value_path}') OR jsonb_path_exists({payload}, '{object_path}'))");
    let sql = present_and(&present, &value_sql);
    Condition::all().add(SeaExpr::cust_with_values(sql, [SeaValue::from(pattern.as_str())]))
}

/// The jsonpath reaching this attribute's `terminal_iri` value in the payload,
/// adapting the parsed [`AttrPath`] onto the shared [`attribute_value_jsonpath`]
/// so the sub-attribute and compound-value cases this fallback needs share the
/// one construction the geo and ordering builders use.
fn value_extraction_jsonpath(path: &AttrPath, terminal_iri: &str) -> String {
    let sub_attrs: Vec<&str> = path.sub_attrs.iter().map(|sub| sub.as_str()).collect();
    let value_path: Vec<&str> = path.value_path.iter().map(String::as_str).collect();
    attribute_value_jsonpath(path.attr.as_str(), &sub_attrs, &value_path, terminal_iri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attribute_name_id::AttributeNameId,
        q_predicate::{attribute_names::ResolvedAttributeNames, projection::compile},
        schema::Entities,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser, predicate::Predicate, scalar::Scalar};
    use sea_query::{PostgresQueryBuilder, Query};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameId::from(7)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameId::from(12),
            ),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameId::from(15)),
        ])
    }

    fn render_condition(condition: Condition) -> String {
        let (sql, _) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        sql
    }

    fn render(q: &str) -> String {
        let expr = parser::parse(q).expect("parse q");
        render_condition(compile(&expr, &QFilterTarget::entities(), &names()).expect("compile q"))
    }

    #[test]
    fn integers_beyond_f64_precision_fall_back_to_the_payload() {
        // The projection stores `double precision`; JSONB stores exact numeric.
        // Probing the projection here would answer from the rounded copy, so the
        // entity would be visible via GET but denied by the filter.
        let sql = render(&format!("\"{SPEED}\"==12345678901234567"));
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn a_range_bound_beyond_f64_precision_falls_back() {
        let sql = render(&format!("\"{SPEED}\"==1..12345678901234567"));
        assert!(sql.contains("jsonb_path_query_first"), "one lossy bound taints the range: {sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn sub_attribute_predicate_falls_back_to_jsonpath() {
        let sql = render("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("attribute_current"), "sub-attributes are not projected: {sql}");
    }

    #[test]
    fn compound_value_path_falls_back_to_jsonpath_with_terminal_key() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(sql.contains(".\\\"city\\\"") || sql.contains(".\"city\""), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn existence_with_a_sub_path_tests_the_whole_path() {
        let sql = render("\"https://example.com/address\".\"https://example.com/city\"");
        assert!(sql.contains("jsonb_path_exists"), "{sql}");
        assert!(sql.contains("city"), "the sub-path must not be silently dropped: {sql}");
    }

    #[test]
    fn fallback_escapes_apostrophes_in_attribute_iris() {
        let path = AttrPath {
            attr: "https://example.com/o'brien".parse().expect("valid attribute name"),
            sub_attrs: vec!["https://example.com/sub".parse().expect("valid attribute name")],
            value_path: Vec::new(),
            match_all_languages: false,
        };
        let expr = Expr::Pred(Predicate::Equality {
            path,
            membership: SetMembership::Within,
            rhs: CompareValue::Scalar(Scalar::Str("x".to_string())),
        });
        let condition = compile(&expr, &QFilterTarget::entities(), &names()).expect("compile");
        let sql = render_condition(condition);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }
}
