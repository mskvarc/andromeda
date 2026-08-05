//! JSONB jsonpath fallback for the `q` predicate shapes the `attribute_current`
//! projection cannot answer.
//!
//! The projection stores one typed value per attribute instance. Sub-attributes
//! (`temperature.observedAt`) and the interior of compound values
//! (`address[city]`) live only inside the payload, so predicates addressing them
//! read the payload directly. This path is unindexed by construction.

use crate::{
    error::RenderError,
    jsonb::{comparison_operand, escape_jsonpath, escape_jsonpath_key},
    operator::{comparison, pattern_match, present_and},
    q_predicate::target::QFilterTarget,
};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{
        path::AttrPath,
        predicate::{CmpOp, PatternPolarity, SetMembership},
        scalar::CompareValue,
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

/// Compile a comparison predicate against the payload.
pub fn fallback_compare(path: &AttrPath, op: CmpOp, rhs: &CompareValue, target: &QFilterTarget<'_>) -> Result<Condition, RenderError> {
    let payload = target.payload_column();
    let value_path = value_extraction_jsonpath(path, VALUE.as_str());
    let object_path = value_extraction_jsonpath(path, OBJECT.as_str());
    let extract_value = format!("jsonb_path_query_first({}, '{}')", payload, escape_jsonpath(&value_path));
    let extract_object = format!("jsonb_path_query_first({}, '{}')", payload, escape_jsonpath(&object_path));

    match rhs {
        CompareValue::Scalar(scalar) => {
            let (cast, value) = comparison_operand(scalar);
            let sql = format!(
                "(COALESCE(({extract_value} #>> '{{}}'), ({extract_object} #>> '{{}}')){cast} {op} $1{cast})",
                op = comparison(op)
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [value])))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_value) = comparison_operand(min);
            let (_, max_value) = comparison_operand(max);
            // A missing element makes the extraction NULL, and every comparison
            // against NULL is unknown, so presence is required structurally in
            // both polarities (clause 4.9).
            let sql = match op.set_membership().ok_or(QueryError::RangeOperatorUnsupported)? {
                SetMembership::Within => format!("(({extract_value} #>> '{{}}')){cast} >= $1{cast} AND (({extract_value} #>> '{{}}')){cast} <= $2{cast}"),
                SetMembership::Outside => format!("(({extract_value} #>> '{{}}')){cast} < $1{cast} OR (({extract_value} #>> '{{}}')){cast} > $2{cast}"),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_value, max_value])))
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
            let sql = match op.set_membership().ok_or(QueryError::ValueListOperatorUnsupported)? {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, binds)))
        }
    }
}

/// Compile a pattern predicate against the payload.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend.
pub fn fallback_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, target: &QFilterTarget<'_>) -> Condition {
    let payload = target.payload_column();
    let jsonpath = escape_jsonpath(&value_extraction_jsonpath(path, VALUE.as_str()));
    let value_sql = format!(
        "(jsonb_path_query_first({payload}, '{jsonpath}')#>>'{{}}')::text {} $1",
        pattern_match(polarity)
    );
    let sql = present_and(&format!("jsonb_path_exists({payload}, '{jsonpath}')"), &value_sql);
    Condition::all().add(SeaExpr::cust_with_values(sql, [SeaValue::from(pattern.as_str())]))
}

/// Build the jsonpath that reaches an attribute's value inside the expanded
/// payload: `$[0]."<attr>"[0]."<sub>"…[0]."<terminal>"[0]."@value"."<key>"…`.
fn value_extraction_jsonpath(path: &AttrPath, terminal_iri: &str) -> String {
    let mut jsonpath = format!("$[0].\"{}\"", escape_jsonpath_key(path.attr.as_str()));
    for sub in &path.sub_attrs {
        jsonpath.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub.as_str())));
    }
    jsonpath.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(terminal_iri)));
    let value_key = if terminal_iri == OBJECT.as_str() { "@id" } else { "@value" };
    jsonpath.push_str(&format!(".\"{value_key}\""));
    for key in &path.value_path {
        jsonpath.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
    }
    jsonpath
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
        };
        let expr = Expr::Pred(Predicate::Compare {
            path,
            op: CmpOp::Equal,
            rhs: CompareValue::Scalar(Scalar::Str("x".to_string())),
        });
        let condition = compile(&expr, &QFilterTarget::entities(), &names()).expect("compile");
        let sql = render_condition(condition);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }
}
