//! `q` filtering over a registration's `additional_properties` JSONB column.
//!
//! Registrations carry no attribute projection, so every predicate reads the
//! JSONB document directly.

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        path::AttrPath,
        predicate::{CmpOp, PatternPolarity, Predicate, SetMembership, present_and},
        scalar::CompareValue,
    },
    sql::literal::escape_single_quoted,
};
use sea_query::{Condition, Expr as SeaExpr};

/// Compile a parsed `q` expression against `additional_properties`.
pub fn csr_expr_to_condition(expr: &Expr) -> Result<Condition, QueryError> {
    match expr {
        Expr::And(terms) => terms.iter().try_fold(Condition::all(), |cond, term| Ok(cond.add(csr_expr_to_condition(term)?))),
        Expr::Or(terms) => terms.iter().try_fold(Condition::any(), |cond, term| Ok(cond.add(csr_expr_to_condition(term)?))),
        Expr::Pred(pred) => csr_pred_to_condition(pred),
    }
}

/// Compile one leaf predicate against `additional_properties`.
fn csr_pred_to_condition(pred: &Predicate) -> Result<Condition, QueryError> {
    match pred {
        Predicate::Exists(path) => Ok(Condition::all().add(SeaExpr::cust_with_values("\"additional_properties\" ? $1", [path.attr.as_str().to_string()]))),
        Predicate::Compare { path, op, rhs } => csr_compare_to_condition(path, *op, rhs),
        Predicate::MatchPattern { path, pattern } => Ok(csr_pattern_condition(path, pattern.as_str(), PatternPolarity::Match)),
        Predicate::NotMatchPattern { path, pattern } => Ok(csr_pattern_condition(path, pattern.as_str(), PatternPolarity::NotMatch)),
    }
}

/// Compile a pattern predicate against `additional_properties`.
///
/// Both polarities require the property to be present (clause 4.9), so both
/// build through [`present_and`].
fn csr_pattern_condition(path: &AttrPath, pattern: &str, polarity: PatternPolarity) -> Condition {
    let extract = csr_value_extract(path);
    let value_sql = format!("({})::text {} $1", extract, polarity.operator());
    Condition::all().add(SeaExpr::cust_with_values(
        present_and(&format!("{extract} IS NOT NULL"), &value_sql),
        [pattern.to_string()],
    ))
}

/// Build a SQL expression that extracts the value of a CSR property from additional_properties.
fn csr_value_extract(path: &AttrPath) -> String {
    let mut json_path = format!("\"additional_properties\"->'{}'", escape_single_quoted(path.attr.as_str()));

    // Unwrap Property wrapper: use ->'value' if it's a Property
    json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);

    for sub in &path.sub_attrs {
        let prev = json_path;
        json_path = format!("({prev})->'{}'", escape_single_quoted(sub.as_str()));
        json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);
    }

    json_path
}

/// Compile a comparison predicate against `additional_properties`.
fn csr_compare_to_condition(path: &AttrPath, op: CmpOp, rhs: &CompareValue) -> Result<Condition, QueryError> {
    let extract = csr_value_extract(path);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let (cast, value) = scalar.as_sql_text();
            let value_sql = format!(
                "(({extract}) #>> '{{}}'){cast} {op_str} $1{cast}",
                extract = extract,
                cast = cast,
                op_str = op.sql_operator(),
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(present_and(&format!("({extract}) IS NOT NULL"), &value_sql), [value])))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_val) = min.as_sql_text();
            let (_, max_val) = max.as_sql_text();
            let value_sql = match op.set_membership().ok_or(QueryError::RangeOperatorUnsupported)? {
                SetMembership::Within => format!(
                    "(({extract}) #>> '{{}}'){cast} >= $1{cast} AND (({extract}) #>> '{{}}'){cast} <= $2{cast}",
                    extract = extract,
                    cast = cast,
                ),
                SetMembership::Outside => format!(
                    "((({extract}) #>> '{{}}'){cast} < $1{cast} OR (({extract}) #>> '{{}}'){cast} > $2{cast})",
                    extract = extract,
                    cast = cast,
                ),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(
                present_and(&format!("({extract}) IS NOT NULL"), &value_sql),
                [min_val, max_val],
            )))
        }
        CompareValue::ValueList(values) => {
            let mut parts = Vec::with_capacity(values.len());
            let mut bind_values = Vec::with_capacity(values.len());
            for (i, v) in values.iter().enumerate() {
                let (cast, val) = v.as_sql_text();
                parts.push(format!(
                    "(({extract}) #>> '{{}}'){cast} = ${idx}{cast}",
                    extract = extract,
                    cast = cast,
                    idx = i + 1
                ));
                bind_values.push(val);
            }
            let joined = parts.join(" OR ");
            let value_sql = match op.set_membership().ok_or(QueryError::ValueListOperatorUnsupported)? {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(
                present_and(&format!("({extract}) IS NOT NULL"), &value_sql),
                bind_values,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{parser, scalar::Scalar};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn render(q: &str) -> String {
        let expr = parser::parse(q).expect("parse q");
        let condition = csr_expr_to_condition(&expr).expect("compile q");
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn existence_probes_the_jsonb_key() {
        let sql = render("temperature");
        assert!(sql.contains("\"additional_properties\" ?"), "{sql}");
    }

    #[test]
    fn a_comparison_unwraps_the_property_envelope() {
        let sql = render("temperature>10");
        assert!(sql.contains("'Property'"), "{sql}");
        assert!(sql.contains(" > "), "{sql}");
    }

    /// An apostrophe cannot appear in a short attribute name — the clause 5.2.4
    /// grammar excludes it — but it is a legal sub-delimiter inside an IRI, so
    /// an expanded name can still carry one into a single-quoted SQL literal.
    #[test]
    fn an_apostrophe_in_a_property_name_is_doubled() {
        let sql = render("\"https://example.com/o'brien\">10");
        assert!(sql.contains("o''brien"), "{sql}");
        assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    #[test]
    fn a_range_after_an_ordering_operator_is_rejected() {
        let expr = parser::parse("temperature>10..20").ok();
        // `>` takes a scalar in the grammar, so the range form only reaches the
        // compiler when built directly.
        assert!(
            expr.is_none()
                || csr_expr_to_condition(&Expr::Pred(Predicate::Compare {
                    path: AttrPath::simple("temperature".parse().expect("valid attribute name")),
                    op: CmpOp::Greater,
                    rhs: CompareValue::Range {
                        min: Scalar::Number(10.into()),
                        max: Scalar::Number(20.into()),
                    },
                }))
                .is_err()
        );
    }

    /// Clause 4.9: `notPatternOp` requires the entity to contain the target
    /// element. This renderer used to disjoin the presence test, so a
    /// registration that simply did not declare the property satisfied every
    /// negated pattern over it.
    #[test]
    fn a_negated_csr_pattern_requires_the_property_to_be_present() {
        let sql = render("name!~=\"^A.*\"");
        assert!(sql.contains("IS NOT NULL AND"), "{sql}");
        assert!(!sql.contains("IS NULL OR"), "{sql}");
        assert!(sql.contains("!~"), "{sql}");
    }

    /// The same rule for `!=` over a range and over a value list, which had the
    /// same defect.
    #[test]
    fn a_negated_csr_comparison_requires_the_property_to_be_present() {
        for expression in ["speed!=10..20", "color!=\"red\",\"blue\"", "speed!=10"] {
            let sql = render(expression);
            assert!(sql.contains("IS NOT NULL AND"), "{expression}: {sql}");
            assert!(!sql.contains("IS NULL OR"), "{expression}: {sql}");
        }
    }
}
