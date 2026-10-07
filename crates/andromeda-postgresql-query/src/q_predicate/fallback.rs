//! JSONB jsonpath fallback for the `q` predicate shapes the typed value columns
//! cannot answer.
//!
//! The projection stores one typed value per attribute instance. Sub-attributes
//! (`temperature.observedAt`) and the interior of compound values
//! (`address[city]`) live only inside an expanded entity document, so
//! predicates addressing them read that document directly: the payload over
//! current state, a rebuilt one-instance document over history (see
//! [`InstanceScan::document_condition`](crate::q_predicate::instance_scan::InstanceScan::document_condition)).
//! This path is unindexed by construction.

use crate::{
    jsonb::{attribute_value_jsonpath, json_literal_member_jsonpath, lift_comparison},
    operator::{compare, pattern as pattern_predicate},
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
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    jsonb::{jsonb_path_exists, jsonb_path_query_first},
    literal::text_literal,
    operator::present_and,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Func, SimpleExpr};

/// Read a JSONB value out as its whole text: `(jsonb) #>> '{}'`.
///
/// The empty `text[]` path yields the value itself as text, which is what every
/// `q` comparison lifts from.
fn whole_text(jsonb: SimpleExpr) -> SimpleExpr {
    jsonb.json_path_text(text_literal("{}"))
}

/// Compile an existence predicate that carries a sub-path.
///
/// A sub-path existence check asks whether a nested element is present, which
/// only the payload can answer.
#[must_use]
pub fn fallback_exists(path: &AttrPath, document: &SimpleExpr) -> Condition {
    Condition::all().add(value_presence(path, document))
}

/// Whether the Property value this path addresses is present in `document`.
///
/// A trailing path is read both into an expanded Property value and into a
/// `JsonProperty`'s raw JSON, and a member present under either one exists.
fn value_presence(path: &AttrPath, document: &SimpleExpr) -> SimpleExpr {
    let expanded = jsonb_path_exists(document.clone(), &value_extraction_jsonpath(path, VALUE.as_str())); // clone: sea-query owns every operand
    match json_literal_extraction_jsonpath(path) {
        Some(jsonpath) => expanded.or(jsonb_path_exists(document.clone(), &jsonpath)), // clone: sea-query owns every operand
        None => expanded,
    }
}

/// The Property value this path addresses in `document`.
///
/// Clause 4.9 evaluates a trailing path "as a `MemberExpression`" against the
/// seed target value. An expanded Property value and a `JsonProperty`'s raw
/// JSON (clause 4.9 EXAMPLE 11) are both seed target values, and an attribute
/// holds at most one of them, so the two readings coalesce.
fn value_extraction(path: &AttrPath, document: &SimpleExpr) -> SimpleExpr {
    let expanded = payload_extraction(path, VALUE.as_str(), document);
    match json_literal_extraction_jsonpath(path) {
        Some(jsonpath) => Func::coalesce([expanded, jsonb_path_query_first(document.clone(), &jsonpath)]).into(), // clone: sea-query owns every operand
        None => expanded,
    }
}

/// The `jsonb_path_query_first` extraction reaching one member of this
/// attribute in the expanded `document`.
fn payload_extraction(path: &AttrPath, terminal_iri: &str, document: &SimpleExpr) -> SimpleExpr {
    let jsonpath = value_extraction_jsonpath(path, terminal_iri);
    jsonb_path_query_first(document.clone(), &jsonpath) // clone: sea-query owns every operand, and one document is read by several extractions
}

/// Compare a stored text expression against one scalar operand.
///
/// Everything that decides the comparison domain (the cast a number or boolean
/// needs, the guarded `timestamptz` comparison a `DateTime` needs, the
/// normalisation a `Time` needs) lives in [`lift_comparison`], so this fallback
/// and the CSR renderer compare the same way.
fn scalar_predicate(stored_text: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    let lifted = lift_comparison(scalar, stored_text);
    compare(lifted.stored, op, lifted.operand)
}

/// Build a single-scalar equality comparison (`==`/`!=`), `COALESCing` a
/// Property's value with a Relationship's object so either matches.
///
/// Clause 4.9 admits a Relationship's object as a comparison target for the
/// equality operators and requires a Relationship operand to non-match every
/// other operator, which is why the object extraction appears here and nowhere
/// else in this module.
fn equality_scalar_comparison(extract_value: SimpleExpr, extract_object: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    let coalesced = Func::coalesce([whole_text(extract_value), whole_text(extract_object)]);
    scalar_predicate(coalesced.into(), scalar, op)
}

/// Compile an equality predicate (`==`/`!=`) against the payload.
#[must_use]
pub fn fallback_equality(path: &AttrPath, membership: SetMembership, rhs: &CompareValue, document: &SimpleExpr) -> Condition {
    let extract_value = value_extraction(path, document);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let extract_object = payload_extraction(path, OBJECT.as_str(), document);
            Condition::all().add(equality_scalar_comparison(extract_value, extract_object, scalar, membership.as_cmp_op()))
        }
        CompareValue::Range { min, max } => {
            let stored = whole_text(extract_value);
            // Both bounds compare against the one lifted stored side; lifting the
            // maximum only contributes its operand.
            let lower = lift_comparison(min, stored.clone()); // clone: the stored side is compared against both bounds
            let upper_operand = lift_comparison(max, stored).operand;
            let stored_side = lower.stored;
            // A missing element makes the extraction NULL, and every comparison
            // against NULL is unknown, so presence is required structurally in
            // both polarities (clause 4.9).
            match membership {
                SetMembership::Within => Condition::all()
                    .add(compare(stored_side.clone(), CmpOp::GreaterEq, lower.operand)) // clone: the stored side is read by both halves
                    .add(compare(stored_side, CmpOp::LessEq, upper_operand)),
                SetMembership::Outside => Condition::any()
                    .add(compare(stored_side.clone(), CmpOp::Less, lower.operand)) // clone: the stored side is read by both halves
                    .add(compare(stored_side, CmpOp::Greater, upper_operand)),
            }
        }
        CompareValue::ValueList(values) => {
            // A value list is a set of equality comparisons (clause 4.9 Table
            // 4.9-1), so it COALESCEs a Relationship's object in exactly the
            // way the single-scalar arm above does — `compile_equality_on_projection`
            // routes every list arm through this same COALESCE via
            // `scalar_condition`, and the fallback has to agree with it.
            let extract_object = payload_extraction(path, OBJECT.as_str(), document);
            let disjunction = values.iter().fold(Condition::any(), |cond, value| {
                // Each arm reads its own COALESCE of the two extractions.
                let coalesced = Func::coalesce([whole_text(extract_value.clone()), whole_text(extract_object.clone())]);
                let lifted = lift_comparison(value, coalesced.into());
                cond.add(compare(lifted.stored, CmpOp::Equal, lifted.operand))
            });
            match membership {
                SetMembership::Within => Condition::all().add(disjunction),
                SetMembership::Outside => Condition::all().add(disjunction.not()),
            }
        }
    }
}

/// Compile an ordering predicate (`>`/`>=`/`<`/`<=`) against the payload.
///
/// A Relationship's object is not extracted at all. Clause 4.9 requires a
/// Relationship (and a `ListRelationship`) operand to non-match every operator
/// other than equality and inequality: a URI names a resource and carries no
/// order a `<` or `>` could mean. `COALESCing` `hasObject` here would
/// make `owner>"a"` select entities the standard requires it to reject.
#[must_use]
pub fn fallback_ordering(path: &AttrPath, op: OrderingOp, rhs: &Scalar, document: &SimpleExpr) -> Condition {
    let extract_value = value_extraction(path, document);
    Condition::all().add(scalar_predicate(whole_text(extract_value), rhs, op.as_cmp_op()))
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
#[must_use]
pub fn fallback_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, document: &SimpleExpr) -> Condition {
    let extract = whole_text(value_extraction(path, document));
    let value = pattern_predicate(cast_compound(extract, PostgresType::Text), polarity, SeaExpr::val(pattern.portable().as_str()));
    Condition::all().add(present_and(value_presence(path, document), value))
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

/// The jsonpath reaching the member a trailing path names inside a
/// `JsonProperty`'s raw JSON, or `None` when the path has no trailing members.
fn json_literal_extraction_jsonpath(path: &AttrPath) -> Option<String> {
    if path.value_path.is_empty() {
        return None;
    }
    let sub_attrs: Vec<&str> = path.sub_attrs.iter().map(AttributeName::as_str).collect();
    let value_path: Vec<&str> = path.value_path.iter().map(AttributeName::as_str).collect();
    Some(json_literal_member_jsonpath(path.attr.as_str(), &sub_attrs, &value_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::{instance_scan::InstanceScan, projection::compile},
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser, path::QueryPath, predicate::Predicate, scalar::Scalar};
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::default(),
        }
    }

    fn attribute_names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameKey::from(7)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameKey::from(12),
            ),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(15)),
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
        render_condition(compile(&expr, &InstanceScan::Current(&EntityRowRefs::unaliased(None)), &names()))
    }

    /// Clause 4.9: a `q` on the system sub-attribute `observedAt` reaches its
    /// value node directly. It is stored bare (`[{"@type":…,"@value":…}]`), so
    /// the jsonpath must terminate at `@value` with no `hasValue` carrier hop.
    #[test]
    fn sub_attribute_predicate_falls_back_to_jsonpath() {
        let sql = render("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "sub-attributes are not projected: {sql}");
        assert!(
            sql.contains("$[0].\\\"https://example.com/temperature\\\"[0].\\\"https://uri.etsi.org/ngsi-ld/observedAt\\\"[0].\\\"@value\\\"")
                || sql.contains("$[0].\"https://example.com/temperature\"[0].\"https://uri.etsi.org/ngsi-ld/observedAt\"[0].\"@value\""),
            "a system sub-attribute reaches @value directly: {sql}"
        );
        assert!(
            !sql.contains("observedAt\\\"[0].\\\"https://uri.etsi.org/ngsi-ld/hasValue"),
            "no hasValue hop is inserted: {sql}"
        );
    }

    /// Clause 4.9 gives `attr[member]` two readings — a compound value member,
    /// and the target language of a `LanguageProperty` — and the query text
    /// cannot tell them apart, so both are compiled and disjoined. This is the
    /// compound half: the payload jsonpath still carries the member key.
    #[test]
    fn compound_value_path_falls_back_to_jsonpath_with_terminal_key() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(sql.contains(".\\\"city\\\"") || sql.contains(".\"city\""), "{sql}");
        assert!(
            sql.contains("jsonb_build_object('@language'"),
            "the language-map reading is compiled beside it: {sql}"
        );
        assert!(sql.contains(" OR "), "{sql}");
    }

    /// A sub-attribute lives only in the payload, so nothing probes the
    /// projection: the Property-value reading and the `languageMap` reading of
    /// a `LanguageProperty` sub-attribute (clause 4.9) both read the payload.
    /// Clause 4.9 EXAMPLE 11 reads the member of a `JsonProperty`'s raw JSON,
    /// which the expanded-value reading never reaches.
    #[test]
    fn a_trailing_path_also_reads_the_member_of_a_json_property() {
        let json_member = "$[0].\"https://example.com/parkingTickets\"[0].\"https://uri.etsi.org/ngsi-ld/hasJSON\"[0].\"@value\".\"value\"";
        for expression in [
            "\"https://example.com/parkingTickets\"[value]==\"Overstay 60 minutes\"",
            "\"https://example.com/parkingTickets\"[value]>=10",
            "\"https://example.com/parkingTickets\"[value]~=\"Over.*\"",
            "\"https://example.com/parkingTickets\"[value]",
        ] {
            let sql = render(expression);
            assert!(sql.contains(json_member), "{expression}: {sql}");
        }
    }

    #[test]
    fn a_path_without_trailing_members_never_reads_a_json_property() {
        let sql = render("\"https://example.com/speed\"==10");
        assert!(!sql.contains("hasJSON"), "{sql}");
    }

    #[test]
    fn a_sub_attribute_path_compiles_to_the_payload_alone() {
        let sql = render("\"https://example.com/address\".\"https://example.com/sub\"==\"a\"");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
        assert!(sql.contains("hasValue"), "{sql}");
        assert!(sql.contains("hasLanguageMap"), "{sql}");
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
        assert!(
            !sql.contains("hasValue\"[0].\"@value\".\"city\""),
            "the member must not be read out of the value node: {sql}"
        );
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
        // Every arm of a value
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
        let condition = compile(&expr, &InstanceScan::Current(&EntityRowRefs::unaliased(None)), &names());
        let sql = render_condition(condition);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }
}
