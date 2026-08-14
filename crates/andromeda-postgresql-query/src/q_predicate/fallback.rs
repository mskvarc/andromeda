//! JSONB jsonpath fallback for the `q` predicate shapes the
//! `current_attribute_values` projection cannot answer.
//!
//! The projection stores one typed value per attribute instance. Sub-attributes
//! (`temperature.observedAt`) and the interior of compound values
//! (`address[city]`) live only inside the payload, so predicates addressing them
//! read the payload directly. A numeric operand too large to survive the bind
//! path exactly lands here as well. This path is unindexed by construction.

use crate::{
    jsonb::{attribute_value_jsonpath, comparison_sql},
    operator::{comparison, pattern_match},
    q_predicate::target::QFilterTarget,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::{
        path::AttrPath,
        predicate::{CmpOp, OrderingOp, PatternPolarity, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBJECT, VALUE};
use andromeda_postgresql_seaquery::{jsonb::escape_jsonpath, operator::present_and};
use sea_query::{Condition, Expr as SeaExpr, Value as SeaValue};

/// Compile an existence predicate that carries a sub-path.
///
/// A sub-path existence check asks whether a nested element is present, which
/// only the payload can answer. An earlier compiler silently dropped the
/// sub-path here and tested the top-level attribute instead.
pub fn fallback_exists(path: &AttrPath, target: &QFilterTarget) -> Condition {
    let jsonpath = value_extraction_jsonpath(path, VALUE.as_str());
    let sql = format!("jsonb_path_exists({}, '{}')", target.payload_column(), escape_jsonpath(&jsonpath));
    Condition::all().add(SeaExpr::cust(sql))
}

/// The `jsonb_path_query_first` extraction reaching one payload member of this
/// attribute.
fn payload_extraction(path: &AttrPath, terminal_iri: &str, target: &QFilterTarget) -> String {
    let jsonpath = value_extraction_jsonpath(path, terminal_iri);
    format!("jsonb_path_query_first({}, '{}')", target.payload_column(), escape_jsonpath(&jsonpath))
}

/// Build the SQL for a comparison between one stored text expression and one
/// operand, both lifted into the domain the operand's kind is ordered in.
///
/// Everything that decides that domain — the cast a number or boolean needs, the
/// guarded `timestamptz` comparison a `DateTime` needs, the normalisation a
/// `Time` needs — lives in [`comparison_sql`], so this fallback and the CSR
/// renderer compare the same way.
fn scalar_predicate(stored_text: &str, scalar: &Scalar, op: CmpOp) -> (String, String) {
    let lifted = comparison_sql(scalar, stored_text, "$1");
    (
        format!(
            "({stored} {op} {operand})",
            stored = lifted.stored,
            op = comparison(op),
            operand = lifted.operand
        ),
        lifted.value,
    )
}

/// Build the SQL for a single-scalar equality comparison (`==`/`!=`), COALESCing
/// a Property's value with a Relationship's object so either matches.
///
/// Clause 4.9 admits a Relationship's object as a comparison target for the
/// equality operators and requires a Relationship operand to non-match every
/// other operator, which is why the object extraction appears here and nowhere
/// else in this module.
fn equality_scalar_comparison(extract_value: &str, extract_object: &str, scalar: &Scalar, op: CmpOp) -> (String, String) {
    scalar_predicate(&format!("COALESCE(({extract_value} #>> '{{}}'), ({extract_object} #>> '{{}}'))"), scalar, op)
}

/// Compile an equality predicate (`==`/`!=`) against the payload.
pub fn fallback_equality(path: &AttrPath, membership: SetMembership, rhs: &CompareValue, target: &QFilterTarget) -> Condition {
    let extract_value = payload_extraction(path, VALUE.as_str(), target);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let extract_object = payload_extraction(path, OBJECT.as_str(), target);
            let (sql, value) = equality_scalar_comparison(&extract_value, &extract_object, scalar, membership.as_cmp_op());
            Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
        }
        CompareValue::Range { min, max } => {
            let stored_text = format!("({extract_value} #>> '{{}}')");
            let lower = comparison_sql(min, &stored_text, "$1");
            let upper = comparison_sql(max, &stored_text, "$2");
            // A missing element makes the extraction NULL, and every comparison
            // against NULL is unknown, so presence is required structurally in
            // both polarities (clause 4.9).
            let sql = match membership {
                SetMembership::Within => format!(
                    "{stored} >= {lower_operand} AND {stored} <= {upper_operand}",
                    stored = lower.stored,
                    lower_operand = lower.operand,
                    upper_operand = upper.operand
                ),
                SetMembership::Outside => format!(
                    "{stored} < {lower_operand} OR {stored} > {upper_operand}",
                    stored = lower.stored,
                    lower_operand = lower.operand,
                    upper_operand = upper.operand
                ),
            };
            Condition::all().add(SeaExpr::cust_with_values(sql, [lower.value, upper.value]))
        }
        CompareValue::ValueList(values) => {
            // A value list is a set of equality comparisons (clause 4.9 Table
            // 4.9-1), so it COALESCEs a Relationship's object in exactly the
            // way the single-scalar arm above does — `compile_equality_on_projection`
            // routes every list arm through this same COALESCE via
            // `scalar_condition`, and the fallback has to agree with it.
            let extract_object = payload_extraction(path, OBJECT.as_str(), target);
            let stored_text = format!("COALESCE(({extract_value} #>> '{{}}'), ({extract_object} #>> '{{}}'))");
            let mut arms = Vec::with_capacity(values.len());
            let mut binds = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let lifted = comparison_sql(value, &stored_text, &format!("${}", index + 1));
                arms.push(format!("{stored} = {operand}", stored = lifted.stored, operand = lifted.operand));
                binds.push(lifted.value);
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
///
/// A Relationship's object is not extracted at all. Clause 4.9 requires a
/// Relationship (and a ListRelationship) operand to non-match every operator
/// other than equality and inequality: a URI names a resource and carries no
/// order a `<` or `>` could mean. COALESCing `hasObject` in here made
/// `owner>"a"` select entities the standard requires it to reject.
pub fn fallback_ordering(path: &AttrPath, op: OrderingOp, rhs: &Scalar, target: &QFilterTarget) -> Condition {
    let extract_value = payload_extraction(path, VALUE.as_str(), target);
    let (sql, value) = scalar_predicate(&format!("({extract_value} #>> '{{}}')"), rhs, op.as_cmp_op());
    Condition::all().add(SeaExpr::cust_with_values(sql, [value]))
}

/// Compile a pattern predicate against the payload.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend.
///
/// Only a Property's value (`hasValue`) is a target. Clause 4.9 defines the
/// pattern operators over String targets and requires a Relationship operand to
/// non-match every operator other than equality and inequality, so a
/// Relationship's object URI is not matched against a pattern however
/// string-shaped it is.
pub fn fallback_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, target: &QFilterTarget) -> Condition {
    let payload = target.payload_column();
    let value_path = escape_jsonpath(&value_extraction_jsonpath(path, VALUE.as_str()));
    let extract = format!("(jsonb_path_query_first({payload}, '{value_path}')#>>'{{}}')");
    let value_sql = format!("({extract})::text {} $1", pattern_match(polarity));
    let present = format!("jsonb_path_exists({payload}, '{value_path}')");
    let sql = present_and(&present, &value_sql);
    Condition::all().add(SeaExpr::cust_with_values(sql, [SeaValue::from(pattern.as_str())]))
}

/// The jsonpath reaching this attribute's `terminal_iri` value in the payload,
/// adapting the parsed [`AttrPath`] onto the shared [`attribute_value_jsonpath`]
/// so the sub-attribute and compound-value cases this fallback needs share the
/// one construction the geo and ordering builders use.
fn value_extraction_jsonpath(path: &AttrPath, terminal_iri: &str) -> String {
    let sub_attrs: Vec<&str> = path.sub_attrs.iter().map(AttributeName::as_str).collect();
    let value_path: Vec<&str> = path.value_path.iter().map(AttributeName::as_str).collect();
    attribute_value_jsonpath(path.attr.as_str(), &sub_attrs, &value_path, terminal_iri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attribute_name_id::AttributeNameId,
        q_predicate::{attribute_names::ResolvedAttributeNames, projection::compile},
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser, path::QueryPath, predicate::Predicate, scalar::Scalar};
    use andromeda_postgresql_schema::current::Entities;
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
        // The operand can only travel as `double precision`, and beyond 2^53 it
        // rounds. Probing the projection with the rounded operand would make the
        // entity visible via GET but denied by the filter.
        let sql = render(&format!("\"{SPEED}\"==12345678901234567"));
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
    }

    #[test]
    fn a_range_bound_beyond_f64_precision_falls_back() {
        let sql = render(&format!("\"{SPEED}\"==1..12345678901234567"));
        assert!(sql.contains("jsonb_path_query_first"), "one lossy bound taints the range: {sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
    }

    #[test]
    fn sub_attribute_predicate_falls_back_to_jsonpath() {
        let sql = render("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "sub-attributes are not projected: {sql}");
    }

    #[test]
    fn compound_value_path_falls_back_to_jsonpath_with_terminal_key() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(sql.contains(".\\\"city\\\"") || sql.contains(".\"city\""), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
    }

    /// The member hop precedes the `@value` selector, because JSON-LD expansion
    /// of a structured Property value produces IRI-keyed member nodes rather than
    /// one `@value` holding a raw object. The whole jsonpath is pinned: a
    /// `contains` assertion cannot see the two selectors in the wrong order.
    #[test]
    fn a_compound_value_predicate_hops_the_member_before_selecting_its_value() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(
            sql.contains("$[0].\"https://example.com/address\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"city\"[*].\"@value\""),
            "{sql}"
        );
        assert!(!sql.contains("\"@value\".\"city\""), "the member must not be read out of the value node: {sql}");
    }

    #[test]
    fn existence_with_a_sub_path_tests_the_whole_path() {
        let sql = render("\"https://example.com/address\".\"https://example.com/city\"");
        assert!(sql.contains("jsonb_path_exists"), "{sql}");
        assert!(sql.contains("city"), "the sub-path must not be silently dropped: {sql}");
    }

    /// Clause 4.9 requires a Relationship operand to non-match every operator
    /// other than equality and inequality, so the payload fallback must not reach
    /// `hasObject` under an ordering or pattern operator.
    #[test]
    fn ordering_and_pattern_predicates_do_not_read_a_relationship_object() {
        for expression in [
            "\"https://example.com/address\".\"https://example.com/sub\">\"a\"",
            "\"https://example.com/address\".\"https://example.com/sub\"~=\"^a\"",
            "\"https://example.com/address\".\"https://example.com/sub\"!~=\"^a\"",
        ] {
            let sql = render(expression);
            assert!(!sql.contains("hasObject"), "{expression}: {sql}");
            assert!(sql.contains("hasValue"), "{expression}: {sql}");
        }
    }

    #[test]
    fn an_equality_predicate_still_reads_a_relationship_object() {
        let sql = render("\"https://example.com/address\".\"https://example.com/sub\"==\"a\"");
        assert!(sql.contains("hasObject"), "clause 4.9 admits a Relationship object under equality: {sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
    }

    #[test]
    fn a_value_list_still_covers_relationship_objects_in_both_polarities() {
        // Mirrors `projection.rs`'s test of the same name: every arm of a value
        // list is an equality comparison, so each one may match a Relationship's
        // object, not only a Property's value. A sub-attribute path forces the
        // payload fallback rather than the projection.
        for expression in [
            "\"https://example.com/address\".\"https://example.com/sub\"==\"a\",\"b\"",
            "\"https://example.com/address\".\"https://example.com/sub\"!=\"a\",\"b\"",
        ] {
            let sql = render(expression);
            assert!(sql.contains("hasObject"), "{expression}: {sql}");
            assert_eq!(sql.matches("COALESCE").count(), 2, "{expression}: one per list member: {sql}");
        }
    }

    #[test]
    fn fallback_escapes_apostrophes_in_attribute_iris() {
        let path = AttrPath {
            attr: "https://example.com/o'brien".parse().expect("valid attribute name"),
            sub_attrs: vec!["https://example.com/sub".parse().expect("valid attribute name")],
            value_path: Vec::new(),
            value_path_compact: Vec::new(),
            match_all_languages: false,
        };
        let expr = Expr::Pred(Predicate::Equality {
            path: QueryPath::Local(path),
            membership: SetMembership::Within,
            rhs: CompareValue::Scalar(Scalar::Str("x".to_string())),
        });
        let condition = compile(&expr, &QFilterTarget::entities(), &names()).expect("compile");
        let sql = render_condition(condition);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }
}
