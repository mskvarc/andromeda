//! `q` filtering over a registration's `additional_properties` JSONB column.
//!
//! Registrations carry no attribute projection, so every predicate reads the
//! JSONB document directly.

use andromeda_ngsi_ld_query::filter::{
    expression::Expr,
    path::AttrPath,
    predicate::{CmpOp, OrderingOp, PatternPolarity, Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
};
use andromeda_postgresql_query::{
    error::RenderError,
    jsonb::comparison_operand,
    literal::escape_single_quoted,
    operator::{comparison, pattern_match, present_and},
};
use sea_query::{Condition, Expr as SeaExpr};

/// Compile a parsed `q` expression against `additional_properties`.
pub fn csr_expr_to_condition(expr: &Expr) -> Result<Condition, RenderError> {
    match expr {
        Expr::And(terms) => terms.iter().try_fold(Condition::all(), |cond, term| Ok(cond.add(csr_expr_to_condition(term)?))),
        Expr::Or(terms) => terms.iter().try_fold(Condition::any(), |cond, term| Ok(cond.add(csr_expr_to_condition(term)?))),
        Expr::Pred(pred) => csr_pred_to_condition(pred),
    }
}

/// Compile one leaf predicate against `additional_properties`.
fn csr_pred_to_condition(pred: &Predicate) -> Result<Condition, RenderError> {
    match pred {
        Predicate::Exists(path) => Ok(Condition::all().add(SeaExpr::cust_with_values("\"additional_properties\" ? $1", [path.attr.as_str().to_string()]))),
        Predicate::Equality { path, membership, rhs } => Ok(csr_equality_condition(path, *membership, rhs)),
        Predicate::Ordering { path, op, rhs } => Ok(csr_ordering_condition(path, *op, rhs)),
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
    // `#>> '{}'` unwraps the jsonb value to its text form without the surrounding
    // quotes a `::text` cast would keep, so a stored `"Alpha"` is matched as
    // `Alpha` (clause 4.9). Every comparison arm in this file extracts the same
    // way; the pattern arm must agree.
    let value_sql = format!("(({}) #>> '{{}}') {} $1", extract, pattern_match(polarity));
    Condition::all().add(SeaExpr::cust_with_values(
        present_and(&format!("{extract} IS NOT NULL"), &value_sql),
        [pattern.to_string()],
    ))
}

/// Build a SQL expression that extracts the value of a CSR property from
/// `additional_properties`, unwrapping the `Property` value wrapper at each
/// level.
///
/// Each level references the prior expression exactly once, aliased inside a
/// scalar subquery. Inlining the `CASE` directly embedded the prior expression
/// three times per level, so the SQL grew as `~3^depth` — a handful of
/// sub-attributes produced a statement megabytes wide. The subquery makes the
/// growth linear in the sub-attribute depth (which [`parse_attr_path`] also
/// bounds).
fn csr_value_extract(path: &AttrPath) -> String {
    let mut json_path = unwrap_property(&format!("\"additional_properties\"->'{}'", escape_single_quoted(path.attr.as_str())));

    for sub in &path.sub_attrs {
        let stepped = format!("({json_path})->'{}'", escape_single_quoted(sub.as_str()));
        json_path = unwrap_property(&stepped);
    }

    json_path
}

/// Unwrap one NGSI-LD `Property` wrapper, using `expr` exactly once.
///
/// The expression is aliased as `j` in a scalar subquery so the `CASE` reads a
/// column three times rather than re-embedding the whole prior expression,
/// which is what keeps [`csr_value_extract`] linear rather than exponential.
fn unwrap_property(expr: &str) -> String {
    format!("(SELECT CASE WHEN (j->>'type') = 'Property' THEN j->'value' ELSE j END FROM (SELECT {expr} AS j) s)")
}

/// Build the present-guarded SQL and its bind for a single-scalar comparison
/// against `additional_properties`.
fn csr_scalar_comparison(extract: &str, scalar: &Scalar, op: CmpOp) -> (String, String) {
    let (cast, value) = comparison_operand(scalar);
    let value_sql = format!("(({extract}) #>> '{{}}'){cast} {op_str} $1{cast}", op_str = comparison(op));
    (present_and(&format!("({extract}) IS NOT NULL"), &value_sql), value)
}

/// Compile an equality predicate (`==`/`!=`) against `additional_properties`.
fn csr_equality_condition(path: &AttrPath, membership: SetMembership, rhs: &CompareValue) -> Condition {
    let extract = csr_value_extract(path);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let (sql, value) = csr_scalar_comparison(&extract, scalar, membership.as_cmp_op());
            Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_val) = comparison_operand(min);
            let (_, max_val) = comparison_operand(max);
            let value_sql = match membership {
                SetMembership::Within => format!("(({extract}) #>> '{{}}'){cast} >= $1{cast} AND (({extract}) #>> '{{}}'){cast} <= $2{cast}"),
                SetMembership::Outside => format!("((({extract}) #>> '{{}}'){cast} < $1{cast} OR (({extract}) #>> '{{}}'){cast} > $2{cast})"),
            };
            Condition::all().add(SeaExpr::cust_with_values(
                present_and(&format!("({extract}) IS NOT NULL"), &value_sql),
                [min_val, max_val],
            ))
        }
        CompareValue::ValueList(values) => {
            let mut parts = Vec::with_capacity(values.len());
            let mut bind_values = Vec::with_capacity(values.len());
            for (i, v) in values.iter().enumerate() {
                let (cast, val) = comparison_operand(v);
                parts.push(format!("(({extract}) #>> '{{}}'){cast} = ${idx}{cast}", idx = i + 1));
                bind_values.push(val);
            }
            let joined = parts.join(" OR ");
            let value_sql = match membership {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            };
            Condition::all().add(SeaExpr::cust_with_values(
                present_and(&format!("({extract}) IS NOT NULL"), &value_sql),
                bind_values,
            ))
        }
    }
}

/// Compile an ordering predicate (`>`/`>=`/`<`/`<=`) against `additional_properties`.
fn csr_ordering_condition(path: &AttrPath, op: OrderingOp, rhs: &Scalar) -> Condition {
    let extract = csr_value_extract(path);
    let (sql, value) = csr_scalar_comparison(&extract, rhs, op.as_cmp_op());
    Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::{eval::evaluate, parser};
    use andromeda_postgresql_query::{
        attribute_name_id::AttributeNameId,
        q_predicate::{attribute_names::ResolvedAttributeNames, projection::compile, target::QFilterTarget},
        schema::Entities,
    };
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query};

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

    #[test]
    fn a_deep_sub_attribute_path_extracts_through_nested_subqueries_not_exponentially() {
        // Each level references the prior expression once, via an aliased
        // scalar subquery, so the SQL grows linearly with depth. Inlining the
        // CASE embedded the prior expression three times per level, so a handful
        // of sub-attributes produced a statement megabytes wide (~3^depth).
        let shallow = render("a.b>1");
        let deeper = render("a.b.c.d.e>1");
        assert!(deeper.contains("AS j"), "uses the aliased subquery form: {deeper}");
        // Three more levels under linear growth stay within a small multiple;
        // under ~3^depth they would be orders of magnitude larger.
        assert!(deeper.len() < shallow.len() * 6, "growth must be linear: {} vs {}", deeper.len(), shallow.len());
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

    /// Clause 4.9 Table 4.9-1 admits a range only after `==`/`!=`; `>` takes a
    /// single scalar. `temperature>10..20` therefore does not parse, and the
    /// split [`Predicate`] variants make a range-after-ordering predicate
    /// unrepresentable, so it can never reach the compiler.
    #[test]
    fn a_range_after_an_ordering_operator_does_not_parse() {
        assert!(parser::parse("temperature>10..20").is_err());
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

    /// Clause 4.9: a pattern compares against the value's text, not its quoted
    /// JSON. A `::text` cast keeps the surrounding quotes, so a stored `"Alpha"`
    /// would only match a pattern that itself included the quotes. The pattern
    /// arm must unwrap with `#>> '{}'` like every comparison arm does.
    #[test]
    fn a_csr_pattern_matches_the_unwrapped_value_not_its_quoted_json() {
        let sql = render("name~=\"^A.*\"");
        assert!(sql.contains("#>> '{}'"), "{sql}");
        assert!(!sql.contains(")::text "), "{sql}");
        assert!(sql.contains(" ~ "), "{sql}");
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

    // -- one reading of clause 4.9, three renderings ------------------------

    /// An attribute IRI used by the conformance fixtures.
    const SPEED: &str = "https://example.com/speed";
    /// An attribute IRI used by the conformance fixtures.
    const COLOR: &str = "https://example.com/color";
    /// An attribute IRI used by the conformance fixtures.
    const NAME: &str = "https://example.com/name";

    /// An entity carrying one value for each of the three fixture attributes.
    fn present(speed: i64, color: &str, name: &str) -> serde_json::Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            SPEED: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": speed}] }],
            COLOR: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": color}] }],
            NAME: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": name}] }],
        }])
    }

    /// An entity carrying none of the three fixture attributes.
    fn absent() -> serde_json::Value {
        serde_json::json!([{ "@id": "urn:ngsi-ld:Vehicle:A2" }])
    }

    fn names() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameId::from(1)),
            (COLOR.parse().expect("valid attribute name"), AttributeNameId::from(2)),
            (NAME.parse().expect("valid attribute name"), AttributeNameId::from(3)),
        ])
    }

    fn render_entity_condition(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    /// One row of the clause 4.9 conformance table.
    struct Case {
        /// The filter, written against the three fixture attribute IRIs.
        expression: String,
        /// Entities the predicate must select.
        matching: Vec<serde_json::Value>,
        /// Entities carrying the attribute that the predicate must reject.
        non_matching: Vec<serde_json::Value>,
    }

    fn cases() -> Vec<Case> {
        let quoted = |iri: &str| format!("\"{iri}\"");
        vec![
            Case {
                expression: format!("{}>50", quoted(SPEED)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(10, "red", "Alpha")],
            },
            Case {
                expression: format!("{}==10..20", quoted(SPEED)),
                matching: vec![present(15, "red", "Alpha")],
                non_matching: vec![present(25, "red", "Alpha")],
            },
            Case {
                expression: format!("{}!=10..20", quoted(SPEED)),
                matching: vec![present(25, "red", "Alpha")],
                non_matching: vec![present(15, "red", "Alpha")],
            },
            Case {
                expression: format!("{}==\"red\",\"blue\"", quoted(COLOR)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(80, "green", "Alpha")],
            },
            Case {
                expression: format!("{}!=\"red\",\"blue\"", quoted(COLOR)),
                matching: vec![present(80, "green", "Alpha")],
                non_matching: vec![present(80, "red", "Alpha")],
            },
            Case {
                expression: format!("{}~=\"^A.*\"", quoted(NAME)),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: vec![present(80, "red", "Beta")],
            },
            Case {
                expression: format!("{}!~=\"^A.*\"", quoted(NAME)),
                matching: vec![present(80, "red", "Beta")],
                non_matching: vec![present(80, "red", "Alpha")],
            },
            Case {
                expression: quoted(SPEED),
                matching: vec![present(80, "red", "Alpha")],
                non_matching: Vec::new(),
            },
        ]
    }

    /// Clause 4.9: an entity that does not carry the addressed element never
    /// satisfies a predicate over it, in either polarity. The in-memory
    /// evaluator decides that directly; the two SQL renderings decide it
    /// structurally, by conjoining the presence test rather than disjoining it,
    /// which is what these assertions pin.
    #[test]
    fn every_backend_reads_clause_four_nine_the_same_way() {
        for case in cases() {
            let expr = parser::parse(&case.expression).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression));

            for entity in &case.matching {
                assert!(evaluate(&expr, entity), "{} must select {entity}", case.expression);
            }
            for entity in &case.non_matching {
                assert!(!evaluate(&expr, entity), "{} must reject {entity}", case.expression);
            }
            assert!(!evaluate(&expr, &absent()), "{} must reject an entity without the attribute", case.expression);

            let projection_sql = render_entity_condition(
                compile(&expr, &QFilterTarget::entities(), &names()).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression)),
            );
            assert!(
                projection_sql.contains("EXISTS ("),
                "{} must test presence structurally: {projection_sql}",
                case.expression
            );
            assert!(
                !projection_sql.contains("IS NULL OR"),
                "{} must not admit a missing element: {projection_sql}",
                case.expression
            );

            let csr_sql = render_entity_condition(csr_expr_to_condition(&expr).unwrap_or_else(|error| unreachable!("{}: {error}", case.expression)));
            assert!(
                !csr_sql.contains("IS NULL OR"),
                "{} must not admit a registration without the property: {csr_sql}",
                case.expression
            );
        }
    }
}
