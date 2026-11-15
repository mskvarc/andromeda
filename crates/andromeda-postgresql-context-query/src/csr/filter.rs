//! `csf` filtering over a registration's `context_source_properties` JSONB
//! column, the stored Context Source Property document every in-memory `csf`
//! reader evaluates too (clause 4.9).
//!
//! Registrations carry no attribute projection, so every predicate reads the
//! JSONB document directly.

use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
use andromeda_ngsi_ld_query::{
    filter::{
        expression::Expr,
        path::{AttrPath, QueryPath},
        predicate::{OrderingOp, Predicate, SetMembership},
        reading::{comparator::Comparator, operand::Operand, order_relation::OrderRelation, value_test::Polarity},
        scalar::{CompareValue, Scalar},
        trailing_path::TrailingPath,
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_vocabulary::{
    compact::classes::{DATE, DATE_TIME, TIME},
    keyword,
};
use andromeda_postgresql_query::{
    jsonb::lift_comparison,
    operator::{PatternIndexes, compare, pattern as pattern_predicate},
};
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    json_type::{JsonType, jsonb_type_is, jsonb_whole_text},
    jsonb::jsonb_path_query,
    literal::{false_literal, int_literal, text_literal},
    operator::{negate_total, present_and},
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
/// than converted, and the one shape with no registration counterpart (the
/// clause 4.9 `{}` traversal, which needs a Relationship a registration cannot
/// carry) renders as a constant `FALSE` instead of a failure.
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
        Predicate::Exists(_) => Condition::all().add(present(csr_target_element(path))),
        Predicate::Equality { membership, rhs, .. } => csr_equality_condition(path, *membership, rhs),
        Predicate::Ordering { op, rhs, .. } => csr_ordering_condition(path, *op, rhs),
        Predicate::MatchPattern { pattern, .. } => csr_pattern_condition(path, pattern, Polarity::Asserted),
        Predicate::NotMatchPattern { pattern, .. } => csr_pattern_condition(path, pattern, Polarity::Complement),
    }
}

/// A predicate no registration can satisfy.
///
/// The clause 4.9 `{}` axis descends a Relationship into the entity it names.
/// A registration is not an Entity and its Context Source Properties carry no
/// Relationship (clause 5.2.9 `ContextSourceRegistration`), so there is no
/// Relationship to descend and no linked entity to reach: the traversal has no
/// target here, and a predicate with no target does not hold. Rejecting the
/// whole filter instead would be wrong, since the traversal is valid `q` syntax, and
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
/// Both polarities require the property to be present and to be a String
/// (clause 4.9: "If the target value data type is different than String then
/// it shall be considered as not matching"), so both build through
/// [`present_and`].
fn csr_pattern_condition(path: &AttrPath, pattern: &Pattern, polarity: Polarity) -> Condition {
    let extract = csr_value_extract(path);
    // `#>> '{}'` unwraps the jsonb value to its text form without the surrounding
    // quotes a `::text` cast would keep, so a stored `"Alpha"` is matched as
    // `Alpha` (clause 4.9).
    let value = pattern_predicate(jsonb_whole_text(extract.clone()), polarity, pattern, PatternIndexes::None);
    let string = present(extract.clone()).and(jsonb_type_is(extract, JsonType::String));
    Condition::all().add(present_and(string, value))
}

/// The target element a path names in `context_source_properties`, NULL when
/// it is absent: the node of its last (sub-)property, or the compound value
/// member a trailing group steps to.
///
/// Clause 4.9 makes an existence term select what "define[s] the target
/// element ... regardless of any target value", so a property node with no
/// value is present, and a trailing member is present when the value holds it.
fn csr_target_element(path: &AttrPath) -> SimpleExpr {
    match path.members() {
        Some(_) => csr_value_extract(path),
        None => csr_property_node(path),
    }
}

/// The node of the last (sub-)property a path names in
/// `context_source_properties`: the property node, then the node of each
/// sub-property below it.
///
/// Clause 4.9 makes each name after the first of a path "a (sub)Property or
/// (sub)Relationship", and a sub-property is a member of its property's node,
/// beside the node's value.
fn csr_property_node(path: &AttrPath) -> SimpleExpr {
    path.sub_attrs
        .iter()
        .fold(context_source_properties().json_get(text_literal(path.attr.as_str())), |node, sub| {
            node.json_get(text_literal(sub.as_str()))
        })
}

/// Build an expression that extracts the target value a path names from
/// `context_source_properties`: the value its last (sub-)property node holds,
/// unwrapped once, and each compound value member stepped into that value.
/// NULL when any of them is absent.
///
/// The trailing members are stepped afterwards, and without `unwrap_property`: a
/// compound member (`address[city]`) is an ordinary JSON object key inside the
/// already-unwrapped value, not a `Property` node in its own right, so wrapping
/// it in the same `CASE` would test a `type` member the member's own value
/// never carries. Dropping them would compare the whole compound value's JSON
/// text instead of the addressed member.
fn csr_value_extract(path: &AttrPath) -> SimpleExpr {
    path.members()
        .into_iter()
        .flat_map(|members| &members.addressed)
        .fold(unwrap_property(csr_property_node(path)), |value, member| {
            value.json_get(text_literal(member.as_str()))
        })
}

/// Unwrap one NGSI-LD `Property` wrapper, using `expr` exactly once.
///
/// The expression is aliased as `j` in a scalar subquery so the `CASE` reads a
/// column three times rather than embedding the node expression three times.
fn unwrap_property(expr: SimpleExpr) -> SimpleExpr {
    let derived = Query::select().expr_as(expr, Alias::new("j")).to_owned();
    // `j` is read three times (the type test, the unwrapped value, and the
    // fallthrough), so each read builds its own reference to the aliased column.
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

/// The JSONB value and the declared datatype IRI one extracted registration
/// property is compared with `scalar` through, which [`lift_comparison`]
/// lifts.
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
            let datatype = extract.clone().json_get_text(text_literal(keyword::TYPE));
            let declared = [(TimeValueForm::DateTime, DATE_TIME), (TimeValueForm::Date, DATE), (TimeValueForm::Time, TIME)]
                .into_iter()
                .fold(CaseStatement::new(), |case, (form, term)| {
                    let iri = form.iri();
                    case.case(
                        datatype.clone().is_in([text_literal(term), text_literal(iri.as_str())]),
                        text_literal(iri.as_str()),
                    )
                });
            (extract.json_get(text_literal(keyword::VALUE)), declared.into())
        }
        Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) => (extract, typed_null(PostgresType::Text)),
    }
}

/// Build the present-guarded single-scalar comparison against
/// `context_source_properties`.
fn csr_scalar_comparison(extract: SimpleExpr, scalar: &Scalar, comparator: Comparator) -> SimpleExpr {
    let guard = present(extract.clone());
    let (text, datatype) = csr_reading(extract, scalar);
    let lifted = lift_comparison(&Operand::from(scalar), text, datatype);
    present_and(guard, compare(lifted.stored, comparator, lifted.operand))
}

/// Compile an equality predicate (`==`/`!=`) against `context_source_properties`.
fn csr_equality_condition(path: &AttrPath, membership: SetMembership, rhs: &CompareValue) -> Condition {
    let extract = csr_value_extract(path);

    match rhs {
        CompareValue::Scalar(scalar) => csr_membership_condition(extract, membership, slice::from_ref(scalar)),
        CompareValue::Range { min, max } => {
            let guard = present(extract.clone());
            let (stored, datatype) = csr_reading(extract, min);
            let lower = lift_comparison(&Operand::from(min), stored.clone(), datatype.clone());
            let upper_operand = lift_comparison(&Operand::from(max), stored, datatype).operand;
            let stored_side = lower.stored;
            let within = compare(stored_side.clone(), Comparator::Ordered(OrderRelation::GreaterOrEqual), lower.operand).and(compare(
                stored_side,
                Comparator::Ordered(OrderRelation::LessOrEqual),
                upper_operand,
            ));
            // Outside is the total negation of within over a present property: a
            // value of another data type lifts to NULL, and clause 4.9 makes it
            // unequal to the range.
            let value = match membership {
                SetMembership::Within => within,
                SetMembership::Outside => negate_total(within),
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
    let guard = present(extract.clone());
    let element = || SimpleExpr::from(SeaExpr::col((Alias::new(ELEMENT), Alias::new(ELEMENT))));
    let equal_to_one = values.iter().fold(Condition::any(), |cond, value| {
        let (stored, datatype) = csr_reading(element(), value);
        let lifted = lift_comparison(&Operand::from(value), stored, datatype);
        cond.add(compare(lifted.stored, Comparator::Equal, lifted.operand))
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
    Condition::all().add(csr_scalar_comparison(extract, rhs, Comparator::Ordered(op.relation())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser, reading::filter_reading::FilterReading};
    use andromeda_postgresql_query::{
        entity_row_refs::EntityRowRefs,
        q_predicate::{instance_scan::InstanceScan, projection::compile},
        query_names::QueryNames,
    };
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, literal::int_literal};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use andromeda_q_corpus::conformance::{COLOR, CONFORMANCE, NAME, SPEED};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, SelectStatement};

    fn render(q: &str) -> SelectStatement {
        let expr = parser::parse(q).expect("parse q");
        let condition = csr_expr_to_condition(&expr);
        Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(condition)
            .to_owned()
    }

    /// Clause 4.9: an existence term selects what defines the target element
    /// its whole path names, so a sub-property and a compound member are read
    /// as a comparison reads them.
    #[test]
    fn existence_reads_the_target_element_of_the_whole_path() {
        assert_statement_snapshot!("filter-existence-reads-the-target-element-of-the-whole-path-1", render("temperature"));
        assert_statement_snapshot!("filter-existence-reads-the-target-element-of-the-whole-path-2", render("a.b"));
        assert_statement_snapshot!("filter-existence-reads-the-target-element-of-the-whole-path-3", render("a[m]"));
    }

    #[test]
    fn a_comparison_unwraps_the_property_envelope() {
        assert_statement_snapshot!("filter-a-comparison-unwraps-the-property-envelope", render("temperature>10"));
    }

    /// A sub-property is a member of its property's node beside its value, so
    /// every level steps a node and only the last node's value is unwrapped,
    /// however deep the path.
    #[test]
    fn a_sub_property_path_steps_the_property_nodes_and_unwraps_one_value() {
        let shallow = render("a.b>1");
        let deeper = render("a.b.c.d.e>1");
        assert_statement_snapshot!("filter-a-sub-property-path-steps-the-property-nodes-and-unwraps-one-value", deeper);
        let unwraps = |statement: &SelectStatement| statement.to_string(PostgresQueryBuilder).matches("CASE").count();
        pretty_assertions::assert_eq!(unwraps(&deeper), unwraps(&shallow));
    }

    /// An apostrophe cannot appear in a short attribute name (the clause 5.2.4
    /// grammar excludes it), but it is a legal sub-delimiter inside an IRI, so
    /// an expanded name can still carry one into a single-quoted SQL literal.
    #[test]
    fn an_apostrophe_in_a_property_name_is_doubled() {
        assert_statement_snapshot!(
            "filter-an-apostrophe-in-a-property-name-is-doubled",
            render("\"https://example.com/o'brien\">10")
        );
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
        assert_statement_snapshot!("filter-a-negated-csr-pattern-requires-the-property-to-be-present", render("name!~=\"^A.*\""));
    }

    /// Clause 4.9: a pattern compares against the value's text, not its quoted
    /// JSON. A `::text` cast keeps the surrounding quotes, so a stored `"Alpha"`
    /// would only match a pattern that itself included the quotes. The pattern
    /// arm must unwrap with `#>> '{}'` like every comparison arm does.
    #[test]
    fn a_csr_pattern_matches_the_unwrapped_value_not_its_quoted_json() {
        assert_statement_snapshot!("filter-a-csr-pattern-matches-the-unwrapped-value-not-its-quoted-json", render("name~=\"^A.*\""));
    }

    /// A compound value's member is a JSON object key inside the unwrapped
    /// value, so it must be stepped without going through `unwrap_property`.
    /// Otherwise the whole compound value's serialized JSON is compared instead
    /// of the addressed member, and `q=info[city]==Berlin` never matches.
    #[test]
    fn a_compound_value_path_steps_into_the_member_without_wrapping_it_as_a_property() {
        // The member step (`->'city'`) is applied right after the sub-attribute
        // extraction's closing parentheses, outside the `unwrap_property` CASE
        // (once for the presence test and once for the comparison), not inside it,
        // which would test a `type` member `city`'s own value never carries.
        assert_statement_snapshot!(
            "filter-a-compound-value-path-steps-into-the-member-without-wrapping-it-as-a-property",
            render("\"https://example.com/address\"[city]==\"Berlin\"")
        );
    }

    /// Table 5.2.9-1 types `expiresAt` as a `DateTime`, which the stored
    /// document declares, so a `dateTime` operand reads its `@value` beside
    /// its `@type` (clause 4.9).
    #[test]
    fn a_date_time_operand_reads_the_declared_value_and_datatype() {
        assert_statement_snapshot!(
            "filter-a-date-time-operand-reads-the-declared-value-and-datatype",
            render("\"https://uri.etsi.org/ngsi-ld/expiresAt\">2030-01-01T00:00:00Z")
        );
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so a number reads only a JSON number and a String only a JSON string.
    #[test]
    fn an_operand_reads_only_a_property_of_its_own_json_type() {
        assert_statement_snapshot!(
            "filter-an-operand-reads-only-a-property-of-its-own-json-type-1",
            render("\"https://example.com/zip\"==3310")
        );
        assert_statement_snapshot!(
            "filter-an-operand-reads-only-a-property-of-its-own-json-type-2",
            render("\"https://example.com/zip\"==\"3310\"")
        );
    }

    /// Clause 4.9: "If the target value data type is different than String
    /// then it shall be considered as not matching", in both polarities.
    #[test]
    fn a_pattern_reads_only_a_string_property() {
        for (index, q) in ["\"https://example.com/zip\"~=\"^33\"", "\"https://example.com/zip\"!~=\"^33\""]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(format!("filter-a-pattern-reads-only-a-string-property-{}", index + 1), render(q));
        }
    }

    /// Clause 4.9: an Equal term holds when its value "is included in the
    /// target value, if the latter is an array", so `operations` is compared
    /// element by element, and a scalar property as the one element it is.
    #[test]
    fn an_equal_term_reads_an_array_property_by_membership() {
        assert_statement_snapshot!(
            "filter-an-equal-term-reads-an-array-property-by-membership",
            render("\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\"")
        );
    }

    /// Clause 4.9: an Unequal term holds when its value "is not included in
    /// the target value, if the latter is an array".
    #[test]
    fn an_unequal_term_holds_when_no_element_of_an_array_property_is_its_value() {
        assert_statement_snapshot!(
            "filter-an-unequal-term-holds-when-no-element-of-an-array-property-is-its-value",
            render("\"https://uri.etsi.org/ngsi-ld/operations\"!=\"queryEntity\"")
        );
    }

    /// Clause 4.9: a value list holds when the array "includes any of the
    /// Query Term values", so every value is compared within one element scan.
    #[test]
    fn a_value_list_reads_an_array_property_in_one_element_scan() {
        assert_statement_snapshot!(
            "filter-a-value-list-reads-an-array-property-in-one-element-scan",
            render("\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\",\"retrieveEntity\"")
        );
    }

    /// The same rule for `!=` over a range and over a value list.
    #[test]
    fn a_negated_csr_comparison_requires_the_property_to_be_present() {
        for (index, expression) in ["speed!=10..20", "color!=\"red\",\"blue\"", "speed!=10"].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("filter-a-negated-csr-comparison-requires-the-property-to-be-present-{}", index + 1),
                render(expression)
            );
        }
    }

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [SPEED, COLOR, NAME]
                    .into_iter()
                    .zip(1..)
                    .map(|(iri, key)| (iri.as_str().parse().expect("valid attribute name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn entity_statement(condition: Condition) -> SelectStatement {
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
    }

    /// Clause 4.9: an entity that does not carry the addressed element never
    /// satisfies a predicate over it, in either polarity. The two SQL renderings
    /// of every shared conformance filter decide it structurally, by conjoining
    /// the presence test rather than disjoining it, which the snapshots of both
    /// renderings show. The one `IS NULL OR` a projection probe holds is the
    /// clause 4.22 expiry guard of the instance, not a test of the addressed
    /// element.
    #[test]
    fn every_conformance_filter_renders_a_projection_probe_and_a_registration_condition() {
        for (index, case) in CONFORMANCE.iter().enumerate() {
            let expr = parser::parse(case.filter).unwrap_or_else(|error| panic!("{}: {error}", case.filter));

            let projection = entity_statement(compile(
                &FilterReading::lowered(&expr, LanguageMatching::TagEquality),
                &InstanceScan::Current(&EntityRowRefs::unaliased(None)),
                &names(),
            ));
            assert_statement_snapshot!(format!("filter-every-backend-reads-clause-four-nine-the-same-way-{}-1", index + 1), projection);
            assert_statement_snapshot!(
                format!("filter-every-backend-reads-clause-four-nine-the-same-way-{}-2", index + 1),
                entity_statement(csr_expr_to_condition(&expr))
            );
        }
    }
}
