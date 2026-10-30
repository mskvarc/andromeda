//! `csf` filtering over a registration's `context_source_properties` JSONB
//! column, the stored Context Source Property document every in-memory `csf`
//! reader evaluates too (clause 4.9).
//!
//! Registrations carry no attribute projection, so every predicate reads the
//! JSONB document directly.

use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
use andromeda_ngsi_ld_query::filter::{
    expression::Expr,
    path::{AttrPath, QueryPath},
    predicate::{CmpOp, OrderingOp, PatternPolarity, Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
    trailing_path::TrailingPath,
};
use andromeda_ngsi_ld_vocabulary::{
    compact::classes::{DATE, DATE_TIME, TIME},
    keyword,
};
use andromeda_postgresql_query::{
    jsonb::lift_comparison,
    operator::{compare, pattern as pattern_predicate},
};
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    jsonb::{jsonb_key_exists, jsonb_path_query},
    literal::{false_literal, int_literal, text_literal},
    operator::present_and,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Query, QueryStatementBuilder, SimpleExpr};
use std::slice;

/// The alias one element of a property is read under, which is also the name
/// of its one column.
const ELEMENT: &str = "element";

/// The lax jsonpath reaching each element of an array, and a value that is not
/// an array as itself: lax mode wraps a non-array in a one-element array before
/// an array accessor applies.
const EVERY_ELEMENT: &str = "$[*]";

/// The registration's `context_source_properties` JSONB document, read fresh
/// each time it opens an extraction.
fn context_source_properties() -> SimpleExpr {
    SeaExpr::col(Alias::new("context_source_properties"))
}

/// Compile a parsed `q` expression against `context_source_properties`.
///
/// The compilation is infallible, which is why it reports no error: a
/// registration property is addressed as a JSONB key rather than resolved
/// against a name dictionary, every operand is bound as a parameter rather
/// than converted, and the one shape with no registration counterpart — the
/// clause 4.9 `{}` traversal, which needs a Relationship a registration cannot
/// carry — renders as a constant `FALSE` instead of a failure.
#[must_use]
pub fn csr_expr_to_condition(expr: &Expr) -> Condition {
    match expr {
        Expr::And(terms) => terms.iter().fold(Condition::all(), |cond, term| cond.add(csr_expr_to_condition(term))),
        Expr::Or(terms) => terms.iter().fold(Condition::any(), |cond, term| cond.add(csr_expr_to_condition(term))),
        Expr::Pred(pred) => csr_pred_to_condition(pred),
    }
}

/// Compile one leaf predicate against `context_source_properties`.
fn csr_pred_to_condition(pred: &Predicate) -> Condition {
    let QueryPath::Local(path) = pred.path() else {
        return no_registration_matches();
    };
    // A target language only a tag or range spells reads a `LanguageProperty`,
    // and Context Source Properties are Properties (clause 5.2.9), so it has
    // no target here.
    if matches!(path.trailing, Some(TrailingPath::Language(_))) {
        return no_registration_matches();
    }
    match pred {
        Predicate::Exists(_) => Condition::all().add(jsonb_key_exists(context_source_properties(), SeaExpr::val(path.attr.as_str()))),
        Predicate::Equality { membership, rhs, .. } => csr_equality_condition(path, *membership, rhs),
        Predicate::Ordering { op, rhs, .. } => csr_ordering_condition(path, *op, rhs),
        Predicate::MatchPattern { pattern, .. } => csr_pattern_condition(path, pattern.portable().as_str(), PatternPolarity::Match),
        Predicate::NotMatchPattern { pattern, .. } => csr_pattern_condition(path, pattern.portable().as_str(), PatternPolarity::NotMatch),
    }
}

/// A predicate no registration can satisfy.
///
/// The clause 4.9 `{}` axis descends a Relationship into the entity it names.
/// A registration is not an Entity and its Context Source Properties carry no
/// Relationship (clause 5.2.9 `ContextSourceRegistration`), so there is no
/// Relationship to descend and no linked entity to reach: the traversal has no
/// target here, and a predicate with no target does not hold. Rejecting the
/// whole filter instead would be wrong — the traversal is valid `q` syntax, and
/// clause 4.9 only requires the entity to "contain the target element" for a
/// predicate to hold.
fn no_registration_matches() -> Condition {
    Condition::all().add(false_literal())
}

/// The present guard for one extraction: `(extract) IS NOT NULL`.
///
/// A missing property makes the extraction NULL, and every comparison against
/// NULL is unknown, so presence is required structurally in both polarities
/// (clause 4.9).
fn present(extract: SimpleExpr) -> SimpleExpr {
    SeaExpr::expr(extract).is_not_null()
}

/// Compile a pattern predicate against `context_source_properties`.
///
/// Both polarities require the property to be present (clause 4.9), so both
/// build through [`present_and`].
fn csr_pattern_condition(path: &AttrPath, pattern: &str, polarity: PatternPolarity) -> Condition {
    let extract = csr_value_extract(path);
    // `#>> '{}'` unwraps the jsonb value to its text form without the surrounding
    // quotes a `::text` cast would keep, so a stored `"Alpha"` is matched as
    // `Alpha` (clause 4.9). Every comparison arm in this file extracts the same
    // way; the pattern arm must agree.
    let value = pattern_predicate(stored_text(extract.clone()), polarity, SeaExpr::val(pattern)); // clone: the presence guard reads the same extraction
    Condition::all().add(present_and(present(extract), value))
}

/// Build an expression that extracts the value of a CSR property from
/// `context_source_properties`, unwrapping the `Property` value wrapper at each
/// sub-attribute level and then stepping into any compound value members.
///
/// Each sub-attribute level references the prior expression exactly once,
/// aliased inside a scalar subquery. Inlining the `CASE` would embed the prior
/// expression three times per level, growing the SQL as `~3^depth`; the
/// subquery keeps growth linear in the sub-attribute depth (which the `q`
/// parser also bounds).
///
/// The trailing members are stepped afterwards, and without `unwrap_property`: a
/// compound member (`address[city]`) is an ordinary JSON object key inside the
/// already-unwrapped value, not a `Property` node in its own right, so wrapping
/// it in the same `CASE` would test a `type` member the member's own value
/// never carries. Dropping them would compare the whole compound value's JSON
/// text instead of the addressed member.
fn csr_value_extract(path: &AttrPath) -> SimpleExpr {
    let mut json_path = unwrap_property(context_source_properties().json_get(text_literal(path.attr.as_str())));

    for sub in &path.sub_attrs {
        json_path = unwrap_property(json_path.json_get(text_literal(sub.as_str())));
    }

    for member in path.members().into_iter().flat_map(|members| &members.addressed) {
        json_path = json_path.json_get(text_literal(member.as_str()));
    }

    json_path
}

/// Unwrap one NGSI-LD `Property` wrapper, using `expr` exactly once.
///
/// The expression is aliased as `j` in a scalar subquery so the `CASE` reads a
/// column three times rather than re-embedding the whole prior expression,
/// which is what keeps [`csr_value_extract`] linear rather than exponential.
fn unwrap_property(expr: SimpleExpr) -> SimpleExpr {
    let derived = Query::select().expr_as(expr, Alias::new("j")).to_owned();
    // `j` is read three times — the type test, the unwrapped value, and the
    // fallthrough — so each read builds its own reference to the aliased column.
    let j = || SimpleExpr::from(SeaExpr::col(Alias::new("j")));
    let case = CaseStatement::new()
        .case(
            SeaExpr::expr(j().json_get_text(text_literal("type"))).eq(text_literal("Property")),
            j().json_get(text_literal("value")),
        )
        .finally(j());
    let unwrapped = Query::select().expr(case).from_subquery(derived, Alias::new("s")).to_owned();
    SimpleExpr::SubQuery(None, Box::new(unwrapped.into_sub_query_statement()))
}

/// The text form of one extracted registration property, which is the side
/// [`lift_comparison`] lifts: `(extract) #>> '{}'`.
fn stored_text(extract: SimpleExpr) -> SimpleExpr {
    extract.json_path_text(text_literal("{}"))
}

/// The text and the declared datatype IRI one extracted registration property
/// is compared with `scalar` through.
///
/// Only a value object declaring a time value form in its `@type` compares
/// with a time operand (clause 4.9: no equality of data types, no match), so
/// such an operand reads the object's `@value` beside its datatype. A
/// registration property keeps the value a client wrote, so that `@type` is
/// the core context's term for the form or the form's IRI. Every other
/// operand reads the whole value and no datatype.
fn csr_reading(extract: SimpleExpr, scalar: &Scalar) -> (SimpleExpr, SimpleExpr) {
    match scalar {
        Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => {
            let datatype = extract.clone().json_get_text(text_literal(keyword::TYPE)); // clone: the value and the datatype read one extraction
            let declared = [(TimeValueForm::DateTime, DATE_TIME), (TimeValueForm::Date, DATE), (TimeValueForm::Time, TIME)]
                .into_iter()
                .fold(CaseStatement::new(), |case, (form, term)| {
                    let iri = form.iri();
                    case.case(
                        datatype.clone().is_in([text_literal(term), text_literal(iri.as_str())]),
                        text_literal(iri.as_str()),
                    ) // clone: every arm reads the one datatype
                });
            (extract.json_get_text(text_literal(keyword::VALUE)), declared.into())
        }
        Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) => (stored_text(extract), typed_null(PostgresType::Text)),
    }
}

/// Build the present-guarded single-scalar comparison against
/// `context_source_properties`.
fn csr_scalar_comparison(extract: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    let guard = present(extract.clone()); // clone: the value side lifts the same extraction
    let (text, datatype) = csr_reading(extract, scalar);
    let lifted = lift_comparison(scalar, text, datatype);
    present_and(guard, compare(lifted.stored, op, lifted.operand))
}

/// Compile an equality predicate (`==`/`!=`) against `context_source_properties`.
fn csr_equality_condition(path: &AttrPath, membership: SetMembership, rhs: &CompareValue) -> Condition {
    let extract = csr_value_extract(path);

    match rhs {
        CompareValue::Scalar(scalar) => csr_membership_condition(extract, membership, slice::from_ref(scalar)),
        CompareValue::Range { min, max } => {
            let guard = present(extract.clone()); // clone: the bounds lift the same extraction
            let (stored, datatype) = csr_reading(extract, min);
            let lower = lift_comparison(min, stored.clone(), datatype.clone()); // clone: both bounds compare against the one lifted stored side
            let upper_operand = lift_comparison(max, stored, datatype).operand;
            let stored_side = lower.stored;
            let value = match membership {
                SetMembership::Within => compare(stored_side.clone(), CmpOp::GreaterEq, lower.operand).and(compare(stored_side, CmpOp::LessEq, upper_operand)),
                SetMembership::Outside => compare(stored_side.clone(), CmpOp::Less, lower.operand).or(compare(stored_side, CmpOp::Greater, upper_operand)),
            };
            Condition::all().add(present_and(guard, value))
        }
        CompareValue::ValueList(values) => csr_membership_condition(extract, membership, values),
    }
}

/// Compile `==` or `!=` against one operand or a value list.
///
/// Clause 4.9 makes an Equal term hold when the operand "is included in the
/// target value, if the latter is an array", and an Unequal term when it "is
/// not included", so an array property (`operations`, `scope`, `datasetId`) is
/// compared element by element. A property that is not an array is read as the
/// one element it is. `!=` is the complement of `==` over a present property,
/// so a value whose data type differs from the operand's is unequal to it.
fn csr_membership_condition(extract: SimpleExpr, membership: SetMembership, values: &[Scalar]) -> Condition {
    let guard = present(extract.clone()); // clone: the elements are read from the same extraction
    let element = || SimpleExpr::from(SeaExpr::col((Alias::new(ELEMENT), Alias::new(ELEMENT))));
    let equal_to_one = values.iter().fold(Condition::any(), |cond, value| {
        let (stored, datatype) = csr_reading(element(), value);
        let lifted = lift_comparison(value, stored, datatype);
        cond.add(compare(lifted.stored, CmpOp::Equal, lifted.operand))
    });
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_path_query(extract, EVERY_ELEMENT), Alias::new(ELEMENT))
        .cond_where(equal_to_one);
    let included = SeaExpr::exists(scan);
    let value = match membership {
        SetMembership::Within => included,
        SetMembership::Outside => included.not(),
    };
    Condition::all().add(present_and(guard, value))
}

/// Compile an ordering predicate (`>`/`>=`/`<`/`<=`) against `context_source_properties`.
fn csr_ordering_condition(path: &AttrPath, op: OrderingOp, rhs: &Scalar) -> Condition {
    let extract = csr_value_extract(path);
    Condition::all().add(csr_scalar_comparison(extract, rhs, op.as_cmp_op()))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_query::{
        filter::{eval::evaluate, language_matching::LanguageMatching, parser},
        queried_entity::linked_entities::NoLinkedEntities,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::{instance_scan::InstanceScan, projection::compile},
    };
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::literal::int_literal;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query};

    fn render(q: &str) -> String {
        let expr = parser::parse(q).expect("parse q");
        let condition = csr_expr_to_condition(&expr);
        Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn existence_probes_the_jsonb_key() {
        let sql = render("temperature");
        assert!(sql.contains("\"context_source_properties\" ?"), "{sql}");
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
        // scalar subquery, so the SQL grows linearly with depth rather than as
        // ~3^depth.
        let shallow = render("a.b>1");
        let deeper = render("a.b.c.d.e>1");
        assert!(deeper.contains("AS \"j\""), "uses the aliased subquery form: {deeper}");
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
    /// element, so a registration that does not declare the property satisfies
    /// no negated pattern over it.
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
        assert!(sql.contains("#>> ('{}')"), "{sql}");
        assert!(!sql.contains(")::text "), "{sql}");
        assert!(sql.contains(" ~ "), "{sql}");
    }

    /// A compound value's member is a JSON object key inside the unwrapped
    /// value, so it must be stepped without going through `unwrap_property` —
    /// otherwise the whole compound value's serialized JSON is compared instead
    /// of the addressed member, and `q=info[city]==Berlin` never matches.
    #[test]
    fn a_compound_value_path_steps_into_the_member_without_wrapping_it_as_a_property() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        // The member step (`->'city'`) is applied right after the sub-attribute
        // extraction's closing parentheses, outside the `unwrap_property` CASE —
        // once for the presence test, once for the comparison — not inside it,
        // which would test a `type` member `city`'s own value never carries.
        assert!(sql.contains("-> ('city')) IS NOT NULL"), "{sql}");
        assert!(sql.contains("= 'Berlin'"), "{sql}");
        assert_eq!(
            sql.matches("-> ('city')").count(),
            2,
            "the member is stepped once for presence, once for comparison: {sql}"
        );
    }

    /// Table 5.2.9-1 types `expiresAt` as a `DateTime`, which the stored
    /// document declares, so a `dateTime` operand reads its `@value` beside
    /// its `@type` (clause 4.9).
    #[test]
    fn a_date_time_operand_reads_the_declared_value_and_datatype() {
        let sql = render("\"https://uri.etsi.org/ngsi-ld/expiresAt\">2030-01-01T00:00:00Z");
        assert!(sql.contains("->> ('@value')"), "{sql}");
        assert!(sql.contains("->> ('@type')"), "{sql}");
        assert!(sql.contains("'https://uri.etsi.org/ngsi-ld/DateTime'"), "{sql}");
    }

    /// Clause 4.9: an Equal term holds when its value "is included in the
    /// target value, if the latter is an array", so `operations` is compared
    /// element by element, and a scalar property as the one element it is.
    #[test]
    fn an_equal_term_reads_an_array_property_by_membership() {
        let sql = render("\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\"");
        assert!(sql.contains("IS NOT NULL AND EXISTS(SELECT 1 FROM jsonb_path_query("), "{sql}");
        assert!(
            sql.contains("'$[*]') AS \"element\" WHERE (\"element\".\"element\" #>> ('{}')) = 'queryEntity'"),
            "{sql}"
        );
        assert!(!sql.contains("NOT EXISTS"), "{sql}");
    }

    /// Clause 4.9: an Unequal term holds when its value "is not included in
    /// the target value, if the latter is an array".
    #[test]
    fn an_unequal_term_holds_when_no_element_of_an_array_property_is_its_value() {
        let sql = render("\"https://uri.etsi.org/ngsi-ld/operations\"!=\"queryEntity\"");
        assert!(sql.contains("IS NOT NULL AND (NOT EXISTS(SELECT 1 FROM jsonb_path_query("), "{sql}");
    }

    /// Clause 4.9: a value list holds when the array "includes any of the
    /// Query Term values", so every value is compared within one element scan.
    #[test]
    fn a_value_list_reads_an_array_property_in_one_element_scan() {
        let sql = render("\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\",\"retrieveEntity\"");
        assert_eq!(sql.matches("jsonb_path_query(").count(), 1, "{sql}");
        assert!(sql.contains("= 'queryEntity' OR "), "{sql}");
        assert!(sql.contains("= 'retrieveEntity'"), "{sql}");
    }

    /// The same rule for `!=` over a range and over a value list.
    #[test]
    fn a_negated_csr_comparison_requires_the_property_to_be_present() {
        for expression in ["speed!=10..20", "color!=\"red\",\"blue\"", "speed!=10"] {
            let sql = render(expression);
            assert!(sql.contains("IS NOT NULL AND"), "{expression}: {sql}");
            assert!(!sql.contains("IS NULL OR"), "{expression}: {sql}");
        }
    }

    /// An attribute IRI used by the conformance fixtures.
    const SPEED: &str = "https://example.com/speed";
    /// An attribute IRI used by the conformance fixtures.
    const COLOR: &str = "https://example.com/color";
    /// An attribute IRI used by the conformance fixtures.
    const NAME: &str = "https://example.com/name";

    /// The stored state an expanded document decodes to.
    fn stored(document: &serde_json::Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored state")
    }

    /// One Property instance holding `value`.
    fn property(value: &serde_json::Value) -> serde_json::Value {
        serde_json::json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        }])
    }

    /// An entity carrying one value for each of the three fixture attributes.
    fn present(speed: i64, color: &str, name: &str) -> EntityState {
        stored(&serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            SPEED: property(&speed.into()),
            COLOR: property(&color.into()),
            NAME: property(&name.into()),
        }]))
    }

    /// An entity carrying none of the three fixture attributes.
    fn absent() -> EntityState {
        stored(&serde_json::json!([{ "@id": "urn:ngsi-ld:Vehicle:A2", "@type": ["https://example.com/Vehicle"] }]))
    }

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([
                (SPEED.parse().expect("valid attribute name"), AttributeNameKey::from(1)),
                (COLOR.parse().expect("valid attribute name"), AttributeNameKey::from(2)),
                (NAME.parse().expect("valid attribute name"), AttributeNameKey::from(3)),
            ]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
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
        matching: Vec<EntityState>,
        /// Entities carrying the attribute that the predicate must reject.
        non_matching: Vec<EntityState>,
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
            let expr = parser::parse(&case.expression).unwrap_or_else(|error| panic!("{}: {error}", case.expression));

            for entity in &case.matching {
                assert!(
                    evaluate(&expr, entity, NoLinkedEntities, LanguageMatching::TagEquality),
                    "{} must select {}",
                    case.expression,
                    entity.id()
                );
            }
            for entity in &case.non_matching {
                assert!(
                    !evaluate(&expr, entity, NoLinkedEntities, LanguageMatching::TagEquality),
                    "{} must reject {}",
                    case.expression,
                    entity.id()
                );
            }
            assert!(
                !evaluate(&expr, &absent(), NoLinkedEntities, LanguageMatching::TagEquality),
                "{} must reject an entity without the attribute",
                case.expression
            );

            let projection_sql = render_entity_condition(compile(
                &expr,
                &InstanceScan::Current(&EntityRowRefs::unaliased(None)),
                &names(),
                LanguageMatching::TagEquality,
            ));
            assert!(
                projection_sql.contains("EXISTS("),
                "{} must test presence structurally: {projection_sql}",
                case.expression
            );
            // The clause 4.22 guard a probe carries is the one `IS NULL OR` the
            // rendering legitimately holds: it tests the instance's expiry, not
            // the addressed element.
            let unguarded = projection_sql.replace(r#"("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW())"#, "");
            assert!(
                !unguarded.contains("IS NULL OR"),
                "{} must not admit a missing element: {projection_sql}",
                case.expression
            );

            let csr_sql = render_entity_condition(csr_expr_to_condition(&expr));
            assert!(
                !csr_sql.contains("IS NULL OR"),
                "{} must not admit a registration without the property: {csr_sql}",
                case.expression
            );
        }
    }
}
