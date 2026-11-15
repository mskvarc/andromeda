//! A `q` filter reading decided over a document of Properties.
//!
//! Every test requires the target to be present: a missing property makes its
//! extraction NULL, so the presence of the target is conjoined with the test
//! in both polarities (clause 4.9), and the complement of a test is its
//! failure over a present target ([`verdict_condition`]).

use crate::{
    jsonb::lift_comparison,
    operator::{PatternIndexes, compare, pattern as pattern_predicate},
    q_predicate::{
        document::property_path::{PropertyPath, declared_reading},
        verdict_condition::{ComplementTarget, verdict_condition},
    },
};
use andromeda_ngsi_ld_query::filter::reading::{
    comparator::Comparator,
    filter_reading::FilterReading,
    operand::Operand,
    order_relation::OrderRelation,
    scalar_test::ScalarTest,
    term_reading::{TermReading, Test},
    value_test::{ComplementScope, ValueTest},
};
use andromeda_postgresql_seaquery::{
    column_reference::ColumnReference,
    json_type::{JsonType, jsonb_type_is, jsonb_whole_text},
    jsonb::jsonb_path_query,
    literal::{false_literal, int_literal},
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SimpleExpr};

/// The alias one element of a property is read under, which is also the name
/// of its one column.
const ELEMENT: &str = "element";

/// The lax jsonpath reaching each element of an array, and a value that is not
/// an array as itself: lax mode wraps a non-array in a one-element array before
/// an array accessor applies.
const EVERY_ELEMENT: &str = "$[*]";

/// The condition under which the document `document` satisfies `reading`.
#[must_use]
pub fn document_condition(reading: &FilterReading, document: &ColumnReference) -> Condition {
    match reading {
        FilterReading::All(children) => children
            .iter()
            .fold(Condition::all(), |all, child| all.add(document_condition(child, document))),
        FilterReading::Any(children) => children
            .iter()
            .fold(Condition::any(), |any, child| any.add(document_condition(child, document))),
        FilterReading::Term(term) => term_condition(term, document),
    }
}

/// The condition under which `document` satisfies one Query Term: constant
/// `FALSE` when the term has no target in a document.
fn term_condition(term: &TermReading, document: &ColumnReference) -> Condition {
    let Some(path) = PropertyPath::of(term) else {
        return Condition::all().add(false_literal());
    };
    match term.test() {
        Test::Exists => Condition::all().add(present(path.element(document))),
        Test::Value(value) => value_condition(value, path, document),
    }
}

/// The condition under which the target value `path` names in `document` is
/// present and satisfies `value`.
///
/// A pattern's complement ranges over a String target alone: clause 4.9 makes
/// a target of another data type "not matching" under `notPatternOp` too.
fn value_condition(value: &ValueTest, path: PropertyPath<'_>, document: &ColumnReference) -> Condition {
    let target = || path.value(document);
    let verdict = verdict_condition(value, asserted_condition(value.test(), target), |scope| match scope {
        ComplementScope::AnyKind => ComplementTarget::Probed,
        ComplementScope::StringTarget => ComplementTarget::Where(is_string(target())),
    });
    Condition::all().add(present(target())).add(verdict)
}

/// The condition under which the target value `target` reads passes the
/// asserted `test`.
///
/// Clause 4.9 makes an Equal term hold when the operand "is included in the
/// target value, if the latter is an array", so an array property
/// (`operations`, `scope`, `datasetId`) is compared element by element and a
/// property that is not an array as the one element it is. A pattern compares
/// against a String's text, unwrapped with `#>> '{}'` rather than cast, which
/// would keep the quotes of its JSON spelling.
fn asserted_condition(test: &ScalarTest, target: impl Fn() -> SimpleExpr) -> Condition {
    let asserted = match test {
        ScalarTest::EqualsAny(operands) => includes_any(target(), operands),
        ScalarTest::Ordered { relation, operand } => compared(&target, Comparator::Ordered(*relation), operand),
        ScalarTest::Within(interval) => compared(&target, Comparator::Ordered(OrderRelation::GreaterOrEqual), interval.min()).and(compared(
            &target,
            Comparator::Ordered(OrderRelation::LessOrEqual),
            interval.max(),
        )),
        ScalarTest::Matches(pattern) => is_string(target()).and(pattern_predicate(jsonb_whole_text(target()), pattern, PatternIndexes::None)),
        ScalarTest::Unsatisfiable => false_literal(),
    };
    Condition::all().add(asserted)
}

/// Whether some element of the JSONB `value` equals one of `operands`, in one
/// element scan.
fn includes_any(value: SimpleExpr, operands: &[Operand]) -> SimpleExpr {
    let element = || SimpleExpr::from(SeaExpr::col((Alias::new(ELEMENT), Alias::new(ELEMENT))));
    let equal_to_one = operands
        .iter()
        .fold(Condition::any(), |any, operand| any.add(compared(&element, Comparator::Equal, operand)));
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_path_query(value, EVERY_ELEMENT), Alias::new(ELEMENT))
        .cond_where(equal_to_one);
    SeaExpr::exists(scan)
}

/// Compare the JSONB value `value` reads with one operand, both lifted into
/// the domain the operand's kind is ordered in.
fn compared(value: &impl Fn() -> SimpleExpr, comparator: Comparator, operand: &Operand) -> SimpleExpr {
    let (stored, datatype) = declared_reading(value, operand);
    let lifted = lift_comparison(operand, stored, datatype);
    compare(lifted.stored, comparator, lifted.operand)
}

/// Whether a JSONB value is a JSON string.
fn is_string(value: SimpleExpr) -> SimpleExpr {
    jsonb_type_is(value, JsonType::String)
}

/// Whether an extraction found its target: `(extract) IS NOT NULL`.
fn present(extract: SimpleExpr) -> SimpleExpr {
    SeaExpr::expr(extract).is_not_null()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        q_predicate::{document::condition::document_condition, instance_scan::InstanceScan, projection::compile},
        query_names::QueryNames,
    };
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, column_reference::ColumnReference, literal::int_literal};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use andromeda_q_corpus::conformance::{COLOR, CONFORMANCE, NAME, SPEED};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, SelectStatement};

    /// The reading a compiled `csf` holds.
    fn reading(q: &str) -> FilterReading {
        FilterReading::of(&parse(q).expect("the filter parses"), LanguageMatching::TagEquality)
    }

    fn document() -> ColumnReference {
        ColumnReference::bare(Alias::new("context_source_properties"))
    }

    fn render(q: &str) -> SelectStatement {
        Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(document_condition(&reading(q), &document()))
            .to_owned()
    }

    /// Clause 4.9: an existence term selects what defines the target element
    /// its whole path names, so a sub-property and a compound member are read
    /// as a comparison reads them.
    #[test]
    fn existence_reads_the_target_element_of_the_whole_path() {
        for (index, q) in ["temperature", "a.b", "a[m]"].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("document-condition-existence-reads-the-target-element-of-the-whole-path-{}", index + 1),
                render(q)
            );
        }
    }

    #[test]
    fn a_comparison_unwraps_the_property_envelope() {
        assert_statement_snapshot!("document-condition-a-comparison-unwraps-the-property-envelope", render("temperature>10"));
    }

    /// A sub-property is a member of its property's node beside its value, so
    /// every level steps a node and only the last node's value is unwrapped,
    /// however deep the path.
    #[test]
    fn a_sub_property_path_steps_the_property_nodes_and_unwraps_one_value() {
        let shallow = render("a.b>1");
        let deeper = render("a.b.c.d.e>1");
        assert_statement_snapshot!("document-condition-a-sub-property-path-steps-the-property-nodes-and-unwraps-one-value", deeper);
        let unwraps = |statement: &SelectStatement| statement.to_string(PostgresQueryBuilder).matches("CASE").count();
        assert_eq!(unwraps(&deeper), unwraps(&shallow));
    }

    /// A path ending on an instance member's expanded IRI reads it as one more
    /// member of the node, keyed as its writer keyed it.
    #[test]
    fn an_instance_member_is_read_as_a_member_of_the_node() {
        assert_statement_snapshot!(
            "document-condition-an-instance-member-is-read-as-a-member-of-the-node",
            render("a.\"https://uri.etsi.org/ngsi-ld/observedAt\">2030-01-01T00:00:00Z")
        );
    }

    /// An apostrophe cannot appear in a short attribute name (the clause 5.2.4
    /// grammar excludes it), but it is a legal sub-delimiter inside an IRI, so
    /// an expanded name can still carry one into a single-quoted SQL literal.
    #[test]
    fn an_apostrophe_in_a_property_name_is_doubled() {
        assert_statement_snapshot!(
            "document-condition-an-apostrophe-in-a-property-name-is-doubled",
            render("\"https://example.com/o'brien\">10")
        );
    }

    /// Context Source Properties carry no Relationship and no
    /// `LanguageProperty`, so neither a hop nor a target language has a
    /// target in a document.
    #[test]
    fn a_hop_or_a_target_language_satisfies_no_document() {
        for (index, q) in ["owner{name}==\"x\"", "name[en-GB]==\"x\""].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("document-condition-a-hop-or-a-target-language-satisfies-no-document-{}", index + 1),
                render(q)
            );
        }
    }

    /// Clause 4.9 Do not match pattern: the entity shall contain the target
    /// element, and a target of another data type than String is not
    /// matching, so `!~=` negates the pattern over a present String alone.
    #[test]
    fn a_negated_pattern_requires_a_present_string() {
        assert_statement_snapshot!("document-condition-a-negated-pattern-requires-a-present-string", render("name!~=\"^A.*\""));
    }

    /// Clause 4.9: a pattern compares against the value's text, not its quoted
    /// JSON spelling.
    #[test]
    fn a_pattern_matches_the_unwrapped_text_of_a_string() {
        assert_statement_snapshot!("document-condition-a-pattern-matches-the-unwrapped-text-of-a-string", render("name~=\"^A.*\""));
    }

    /// A compound value's member is a JSON key inside the unwrapped value, so
    /// it is stepped outside the unwrapping `CASE`, which would test a `type`
    /// member `city`'s own value never carries.
    #[test]
    fn a_compound_value_path_steps_into_the_member_outside_the_unwrapping() {
        assert_statement_snapshot!(
            "document-condition-a-compound-value-path-steps-into-the-member-outside-the-unwrapping",
            render("\"https://example.com/address\"[city]==\"Berlin\"")
        );
    }

    /// Table 5.2.9-1 types `expiresAt` as a `DateTime`, which the stored
    /// document declares, so a `DateTime` operand reads its `@value` beside its
    /// `@type` (clause 4.9).
    #[test]
    fn a_date_time_operand_reads_the_declared_value_and_datatype() {
        assert_statement_snapshot!(
            "document-condition-a-date-time-operand-reads-the-declared-value-and-datatype",
            render("\"https://uri.etsi.org/ngsi-ld/expiresAt\">2030-01-01T00:00:00Z")
        );
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so a number reads only a JSON number and a String only a JSON string.
    #[test]
    fn an_operand_reads_only_a_property_of_its_own_json_type() {
        for (index, q) in ["\"https://example.com/zip\"==3310", "\"https://example.com/zip\"==\"3310\""]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!("document-condition-an-operand-reads-only-a-property-of-its-own-json-type-{}", index + 1),
                render(q)
            );
        }
    }

    /// Clause 4.9: "If the target value data type is different than String
    /// then it shall be considered as not matching", in both polarities.
    #[test]
    fn a_pattern_reads_only_a_string_property() {
        for (index, q) in ["\"https://example.com/zip\"~=\"^33\"", "\"https://example.com/zip\"!~=\"^33\""]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(format!("document-condition-a-pattern-reads-only-a-string-property-{}", index + 1), render(q));
        }
    }

    /// Clause 4.9: an Equal term holds when its value "is included in the
    /// target value, if the latter is an array", and a value list when the
    /// array "includes any of the Query Term values", so every value is
    /// compared within one element scan.
    #[test]
    fn an_equal_term_reads_an_array_property_by_membership() {
        for (index, q) in [
            "\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\"",
            "\"https://uri.etsi.org/ngsi-ld/operations\"==\"queryEntity\",\"retrieveEntity\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("document-condition-an-equal-term-reads-an-array-property-by-membership-{}", index + 1),
                render(q)
            );
        }
    }

    /// Clause 4.9: an Unequal term holds when its value "is not included in
    /// the target value, if the latter is an array", and over a present
    /// property only, whether it compares with a value, a list or a range.
    #[test]
    fn an_unequal_term_holds_over_a_present_property_that_includes_no_operand() {
        for (index, q) in [
            "\"https://uri.etsi.org/ngsi-ld/operations\"!=\"queryEntity\"",
            "speed!=10..20",
            "color!=\"red\",\"blue\"",
            "speed!=10",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!(
                    "document-condition-an-unequal-term-holds-over-a-present-property-that-includes-no-operand-{}",
                    index + 1
                ),
                render(q)
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
    /// satisfies a predicate over it, in either polarity. The rendering over
    /// attribute rows and the rendering over a document decide it
    /// structurally, by conjoining the presence test rather than disjoining
    /// it, which the snapshots of both show. The one `IS NULL OR` a
    /// projection probe holds is the clause 4.22 expiry guard of the instance,
    /// not a test of the addressed element.
    #[test]
    fn every_conformance_filter_renders_a_projection_probe_and_a_document_condition() {
        for (index, case) in CONFORMANCE.iter().enumerate() {
            let expr = parse(case.filter).unwrap_or_else(|error| panic!("{}: {error}", case.filter));
            let projection = entity_statement(compile(
                &FilterReading::lowered(&expr, LanguageMatching::TagEquality),
                &InstanceScan::Current(&EntityRowRefs::unaliased(None)),
                &names(),
            ));
            assert_statement_snapshot!(format!("document-condition-every-conformance-filter-{}-1", index + 1), projection);
            assert_statement_snapshot!(
                format!("document-condition-every-conformance-filter-{}-2", index + 1),
                entity_statement(document_condition(&FilterReading::of(&expr, LanguageMatching::TagEquality), &document()))
            );
        }
    }
}
