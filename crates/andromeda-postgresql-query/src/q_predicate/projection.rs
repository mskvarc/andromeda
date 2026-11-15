//! `q` predicates compiled to `EXISTS` probes against the stored attribute
//! instances an [`InstanceScan`] names: the `current_attributes` rows for a
//! Query Entities `q`, the in-window `attribute_history` rows for a temporal
//! one.
//!
//! This module routes each term reading by what its path ends on (its
//! [`Terminal`]) to the readings that can hold its target value, and compiles
//! each reading with the comparison of its test (`equality_term`,
//! `ordering_term`, `pattern_term`, and existence here). It descends
//! linked-entity hops first.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::{equals_any_value, within_range},
        instance_row::InstanceRow,
        instance_scan::InstanceScan,
        instance_test::{CompiledTerm, Disjunction, InstanceTest},
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
        ordering_term::ordering_value,
        pattern_term::{pattern_value, string_target},
        resolved_filter::{ResolvedFilter, ResolvedTerm},
        sub_attribute::{element_path::ElementPath, member_field::member_condition},
        traversal::LinkedHop,
        value_object_member::member_term,
        verdict_condition::{ComplementTarget, verdict_condition},
    },
    query_names::QueryNames,
};
use andromeda_ngsi_ld_query::filter::{
    language_matching::LanguageMatching,
    reading::{
        filter_reading::FilterReading,
        scalar_test::ScalarTest,
        term_reading::{TermReading, Test},
        terminal::Terminal,
        value_test::ComplementScope,
    },
    target_language::LanguageSelector,
};
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::Condition;

/// Compile the reading of a `q` filter into a sea-query [`Condition`].
///
/// Compilation cannot fail: a filter whose names the dictionary does not
/// hold folds as [`ResolvedFilter`] states, to a constant `FALSE` where no
/// Entity can match, and every operand is bound as a parameter rather than
/// converted, so every expression clause 4.9 admits has a condition.
///
/// Every Query Term is its own existential over the instances `scan` reads,
/// and the AND/OR tree combines those existentials. Over a temporal window
/// that is clause 5.7.4.4's reading: each term is "checked against all the
/// Attribute instances resulting from the initial filtering", so `a>1;b<2`
/// holds when some in-window instance of `a` exceeds 1 and some in-window
/// instance of `b` is below 2, not necessarily at the same instant.
///
/// The existentials of a disjunction are one probe rather than several, which
/// says the same thing (see [`Disjunction`]) in the one shape the planner can
/// drive from the value indexes.
#[must_use]
pub fn compile(reading: &FilterReading, scan: &InstanceScan<'_>, names: &QueryNames) -> Condition {
    ResolvedFilter::of(reading, names).map_or_else(unsatisfiable, |resolved| compile_resolved(&resolved, scan, None))
}

/// Compile one term reading on its own, as [`compile`] compiles a filter of
/// that one term.
#[must_use]
pub fn compile_term(term: &TermReading, scan: &InstanceScan<'_>, names: &QueryNames) -> Condition {
    ResolvedTerm::of(term, names).map_or_else(unsatisfiable, |resolved| compiled_term(&resolved, scan).into_condition(scan))
}

/// [`compile`], with `answered` (a term of the reading's top-level
/// conjunction the statement answers on its own) compiled as no constraint.
///
/// A conjunct equal to `answered` asks what `answered` asks, so every one is
/// left out, wherever the conjunction repeats it.
#[must_use]
pub fn compile_except(reading: &FilterReading, answered: &TermReading, scan: &InstanceScan<'_>, names: &QueryNames) -> Condition {
    ResolvedFilter::of(reading, names).map_or_else(unsatisfiable, |resolved| compile_resolved(&resolved, scan, Some(answered)))
}

/// Compile `filter`, leaving out the conjuncts equal to `answered`.
fn compile_resolved(filter: &ResolvedFilter<'_>, scan: &InstanceScan<'_>, answered: Option<&TermReading>) -> Condition {
    match filter {
        ResolvedFilter::All(children) => children
            .iter()
            .fold(Condition::all(), |cond, child| cond.add(compile_resolved(child, scan, answered))),
        ResolvedFilter::Any(children) => disjunction(children, Disjunction::default(), scan).into_condition(scan),
        ResolvedFilter::Term(term) if answered.is_some_and(|answered| answered == term.term()) => Condition::all(),
        ResolvedFilter::Term(term) => compiled_term(term, scan).into_condition(scan),
    }
}

/// Gather the children of a disjunction, and of every disjunction nested
/// directly in it, into `gathered`.
fn disjunction(children: &[ResolvedFilter<'_>], gathered: Disjunction, scan: &InstanceScan<'_>) -> Disjunction {
    children.iter().fold(gathered, |gathered, child| match child {
        ResolvedFilter::Any(inner) => disjunction(inner, gathered, scan),
        ResolvedFilter::All(_) => gathered.with(CompiledTerm::Standalone(compile_resolved(child, scan, None))),
        ResolvedFilter::Term(term) => gathered.with(compiled_term(term, scan)),
    })
}

/// Compile one term, descending the linked-entity hops it carries before
/// compiling its test against the entity it lands on.
fn compiled_term(term: &ResolvedTerm<'_>, scan: &InstanceScan<'_>) -> CompiledTerm {
    compile_from_hop(term, scan, 0)
}

/// Compile a term against the entity reached by its hops from the one at
/// `depth` on.
///
/// `depth` is the number of hops already descended; it suffixes the aliases so
/// a nested traversal's probes cannot shadow an enclosing one's.
///
/// The instances `scan` reads decide which entities the Relationship named.
/// Over current state a hop lands on the linked Entity as it stands. Over a
/// temporal window it lands on the linked entity's instances valid within the
/// same window: the `temporal_linked_retrieval` extension renders every Linked
/// Entity over the window of the roots, so a filter asking about a linked
/// entity reads the history the answer shows of it.
fn compile_from_hop(term: &ResolvedTerm<'_>, scan: &InstanceScan<'_>, depth: usize) -> CompiledTerm {
    let Some(hop) = term.hops().get(depth) else {
        return compile_local(term, scan);
    };
    let linked = LinkedHop::at_depth(depth + 1);
    let inner_condition = match scan {
        InstanceScan::Current(_) => linked_condition(term, &linked.inner_rows(scan.schema().cloned()), depth + 1),
        InstanceScan::History(window) => {
            let linked_window = window.over(linked.linked_evolution());
            let linked_scan = InstanceScan::History(&linked_window);
            compile_from_hop(term, &linked_scan, depth + 1).into_condition(&linked_scan)
        }
    };
    CompiledTerm::Standalone(linked.probe(hop.relationship(), hop.guard(), scan, inner_condition))
}

/// What `term` asks, from its hop at `depth` on, of the linked entity `rows`
/// names, reached `depth` hops into a traversal.
pub(crate) fn linked_condition(term: &ResolvedTerm<'_>, rows: &EntityRowRefs, depth: usize) -> Condition {
    let scan = InstanceScan::Current(rows);
    compile_from_hop(term, &scan, depth).into_condition(&scan)
}

/// Compile a term over the instances `scan` reads.
///
/// The target element is held by a row of the attribute, or by an element of
/// the row's `sub_attributes` for a sub-attribute path; a current row and a
/// history row hold both alike. A row's test joins the probe of its attribute,
/// which a disjunction shares; an element's test is a probe of its own (see
/// [`InstanceScan::exists_in_tree`]).
fn compile_local(term: &ResolvedTerm<'_>, scan: &InstanceScan<'_>) -> CompiledTerm {
    let element = ElementPath::of_resolved(term, scan.row());
    let holder = element.holder();
    let test = term.term().test();
    let value = match term.term().terminal() {
        Terminal::OwnValue => own_value(test, holder),
        Terminal::InstanceMember(member) => member_condition(*member, holder, test),
        // One trailing member has a second reading beside the compound
        // member, the target language of a `LanguageProperty`, and the stored
        // instance decides which applies. Neither can match the other's
        // instances (a `languageMap` has no IRI-keyed members, a compound
        // value no `@language`), so both are compiled and disjoined.
        Terminal::ByFamily { language, matching, members } => Condition::any()
            .add(language_reading(test, LanguageSelector::Tag(language), *matching, holder))
            .add(member_term(test, members, holder)),
        Terminal::Members(members) => member_term(test, members, holder),
        Terminal::Language { language, matching } => language_reading(test, language.selector(), *matching, holder),
    };
    if element.chain.is_empty() {
        return CompiledTerm::Instances(InstanceTest {
            attribute: element.attribute,
            value,
        });
    }
    CompiledTerm::Standalone(scan.exists_in_tree(element.attribute, element.condition(value)))
}

/// A test against the instance's own value.
///
/// A complement of `!=` ranges over every instance the probe reads, whatever
/// its value, and one of `!~=` over an instance holding a String target.
fn own_value(test: &Test, row: InstanceRow) -> Condition {
    match test {
        // An instance *is* the attribute: an attribute deleted by clause
        // 5.6.16 leaves no row and no element behind, so presence needs no
        // further guard.
        Test::Exists => Condition::all(),
        Test::Value(value) => verdict_condition(value, own_value_passes(value.test(), row), |scope| match scope {
            ComplementScope::AnyKind => ComplementTarget::Probed,
            ComplementScope::StringTarget => ComplementTarget::Where(string_target(row)),
        }),
    }
}

/// Whether the instance's own value passes `test`, asserted.
fn own_value_passes(test: &ScalarTest, row: InstanceRow) -> Condition {
    match test {
        ScalarTest::EqualsAny(operands) => equals_any_value(operands, row),
        ScalarTest::Within(interval) => within_range(interval, row),
        ScalarTest::Ordered { relation, operand } => ordering_value(*relation, operand, row),
        ScalarTest::Matches(pattern) => pattern_value(pattern, row),
        ScalarTest::Unsatisfiable => unsatisfiable(),
    }
}

/// A test against the strings a `languageMap` holds under the keys one target
/// language selects (clause 4.9).
///
/// Every test is decided string by string over the selected entries, so the
/// key comparison is the one `matching` names, case-insensitive under tag
/// equality as RFC 5646 section 2.1.1 requires. Existence asks for a selected
/// key-value pair, which is the target element of a language-selected path. A
/// number, boolean or time operand matches no string, and clause 4.9 makes the
/// datatype difference a non-match.
fn language_reading(test: &Test, language: LanguageSelector<'_>, matching: LanguageMatching, row: InstanceRow) -> Condition {
    Condition::all().add(language_term(
        test,
        &LanguageMap::of_instance(row),
        LanguageSelection::TargetLanguage(language, matching),
    ))
}

/// A predicate that no row can satisfy.
pub(crate) fn unsatisfiable() -> Condition {
    Condition::all().add(false_literal())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entity_row_refs::EntityRowRefs,
        q_predicate::lowered_filter::{reading_of, term_of},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::parser::parse;
    use andromeda_postgresql_schema::{current::Entities, string_value_column::INDEXED_STRING_BYTE_LIMIT};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use sea_query::{Alias, Query, SelectStatement};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::from_pairs([
                ("https://example.com/Building".parse().expect("valid type name"), EntityTypeNameKey::from(31)),
                ("https://example.com/Store".parse().expect("valid type name"), EntityTypeNameKey::from(32)),
            ]),
            ..QueryNames::default()
        }
    }

    fn attribute_names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameKey::from(7)),
            (
                "https://example.com/brandName".parse().expect("valid attribute name"),
                AttributeNameKey::from(8),
            ),
            ("https://example.com/active".parse().expect("valid attribute name"), AttributeNameKey::from(9)),
            (
                "https://example.com/observed".parse().expect("valid attribute name"),
                AttributeNameKey::from(10),
            ),
            ("https://example.com/d".parse().expect("valid attribute name"), AttributeNameKey::from(11)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameKey::from(12),
            ),
            ("https://example.com/color".parse().expect("valid attribute name"), AttributeNameKey::from(13)),
            ("https://example.com/name".parse().expect("valid attribute name"), AttributeNameKey::from(14)),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(15)),
            ("https://example.com/label".parse().expect("valid attribute name"), AttributeNameKey::from(40)),
            ("https://example.com/a".parse().expect("valid attribute name"), AttributeNameKey::from(16)),
            ("https://example.com/b".parse().expect("valid attribute name"), AttributeNameKey::from(17)),
            ("https://example.com/c".parse().expect("valid attribute name"), AttributeNameKey::from(18)),
            (
                "https://example.com/isParkedIn".parse().expect("valid attribute name"),
                AttributeNameKey::from(19),
            ),
            (
                "https://example.com/category".parse().expect("valid attribute name"),
                AttributeNameKey::from(20),
            ),
            ("https://example.com/outer".parse().expect("valid attribute name"), AttributeNameKey::from(21)),
            ("https://example.com/inner".parse().expect("valid attribute name"), AttributeNameKey::from(22)),
            ("https://example.com/zone".parse().expect("valid attribute name"), AttributeNameKey::from(23)),
        ])
    }

    /// The statement a real query would send for a compiled condition, so the
    /// snapshot shows the final placeholder numbering.
    fn render(q: &str) -> SelectStatement {
        render_with(q, &names(), &EntityRowRefs::unaliased(None))
    }

    fn render_with(q: &str, names: &QueryNames, rows: &EntityRowRefs) -> SelectStatement {
        render_under(q, names, rows, LanguageMatching::TagEquality)
    }

    fn render_under(q: &str, names: &QueryNames, rows: &EntityRowRefs, matching: LanguageMatching) -> SelectStatement {
        let expr = parse(q).expect("parse q");
        let condition = compile(&FilterReading::lowered(&expr, matching), &InstanceScan::Current(rows), names);
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
    }

    fn quoted(iri: &str) -> String {
        format!("\"{iri}\"")
    }

    #[test]
    fn number_comparison_probes_typed_number_column() {
        let statement = render(&format!("{}>50", quoted(SPEED)));
        assert_statement_snapshot!("projection-number-comparison-probes-typed-number-column", statement);
    }

    #[test]
    fn a_number_comparison_binds_numeric_without_a_double_conversion() {
        let statement = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert_statement_snapshot!("projection-a-number-comparison-binds-numeric-without-a-double-conversion", statement);
    }

    #[test]
    fn a_pattern_does_not_reach_relationship_objects() {
        // Clause 4.9 requires a Relationship operand to non-match every operator
        // other than equality and inequality, and the pattern operators are
        // defined over String targets. `value_uri` holds Relationship objects and
        // nothing else, so a pattern must not probe it.
        let statement = render("\"https://example.com/brandName\"~=\"Merc.*\"");
        assert_statement_snapshot!("projection-a-pattern-does-not-reach-relationship-objects", statement);
    }

    #[test]
    fn a_negated_pattern_does_not_reach_relationship_objects_either() {
        let statement = render("\"https://example.com/brandName\"!~=\"Merc.*\"");
        assert_statement_snapshot!("projection-a-negated-pattern-does-not-reach-relationship-objects-either", statement);
    }

    #[test]
    fn equality_covers_property_values_and_relationship_objects() {
        for (index, expression) in [
            "\"https://example.com/brandName\"==\"Mercedes\"",
            "\"https://example.com/brandName\"!=\"Mercedes\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-equality-covers-property-values-and-relationship-objects-{}", index + 1),
                render(expression)
            );
        }
    }

    #[test]
    fn an_ordering_comparison_does_not_reach_relationship_objects() {
        // Clause 4.9: a Relationship operand shall non-match every operator other
        // than equality and inequality. A URI carries no order a `<` could mean.
        for (index, expression) in [
            "\"https://example.com/brandName\">\"Mercedes\"",
            "\"https://example.com/brandName\">=\"Mercedes\"",
            "\"https://example.com/brandName\"<\"Mercedes\"",
            "\"https://example.com/brandName\"<=\"Mercedes\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-an-ordering-comparison-does-not-reach-relationship-objects-{}", index + 1),
                render(expression)
            );
        }
    }

    #[test]
    fn a_string_range_bound_keeps_relationship_objects_out_of_its_ordering_halves() {
        // A range compiles to two ordering comparisons, so it inherits the
        // clause 4.9 gating rather than restating it.
        let statement = render("\"https://example.com/brandName\"==\"a\"..\"z\"");
        assert_statement_snapshot!(
            "projection-a-string-range-bound-keeps-relationship-objects-out-of-its-ordering-halves",
            statement
        );
    }

    #[test]
    fn a_value_list_still_covers_relationship_objects_in_both_polarities() {
        // Every arm of a value list is an equality comparison (clause 4.9
        // Table 4.9-1), so each one may match a Relationship's object.
        for (index, expression) in [
            "\"https://example.com/color\"==\"red\",\"blue\"",
            "\"https://example.com/color\"!=\"red\",\"blue\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-a-value-list-still-covers-relationship-objects-in-both-polarities-{}", index + 1),
                render(expression)
            );
        }
    }

    /// Clause 4.9, `unequal`: "If the data type of the target value and the data
    /// type of the Query Term value are different, then they shall be considered
    /// unequal."
    ///
    /// A row whose value sits in another typed column makes every comparison
    /// unknown, so the negation has to be total: `NOT (…)` would propagate that
    /// unknown and drop the very rows the clause makes matches.
    #[test]
    fn an_inequality_negates_the_whole_equality_condition_totally() {
        let statement = render("\"https://example.com/brandName\"!=\"Mercedes\"");
        assert_statement_snapshot!("projection-an-inequality-negates-the-whole-equality-condition-totally", statement);
    }

    /// A long operand's equality reads only the overflow column, and its
    /// inequality is still the total negation of that equality (clause 4.9
    /// `unequal`), so a short stored value (whose overflow arm is unknown)
    /// is unequal to it rather than dropped.
    #[test]
    fn a_negated_long_string_equality_is_the_total_negation_of_the_overflow_comparison() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let statement = render(&format!("\"https://example.com/name\"!=\"{long}\""));
        assert_statement_snapshot!(
            "projection-a-negated-long-string-equality-is-the-total-negation-of-the-overflow-comparison",
            statement
        );
    }

    /// A string range is two ordering comparisons, and each reads both string
    /// columns under the ICU root collation (clause 4.9 recommends the UCA).
    #[test]
    fn a_string_range_reads_both_string_columns_on_each_bound() {
        let statement = render("\"https://example.com/brandName\"==\"a\"..\"z\"");
        assert_statement_snapshot!("projection-a-string-range-reads-both-string-columns-on-each-bound", statement);
    }

    /// Clause 4.9 follows every object of the Relationship: one object in
    /// either URI column, several as the JSON array in `value_object`, each
    /// expanded to one row and joined to the entity it names.
    #[test]
    fn a_hop_expands_every_object_of_the_relationship() {
        let statement = render("\"https://example.com/isParkedIn\"{\"https://example.com/name\"}==\"x\"");
        assert_statement_snapshot!("projection-a-hop-expands-every-object-of-the-relationship", statement);
    }

    /// Clause 4.9: an operand is matched when it is included in the target value
    /// of a Relationship holding several objects, the JSON array of URIs in
    /// `value_object`.
    #[test]
    fn a_relationship_equality_reads_an_object_among_several() {
        let statement = render("\"https://example.com/isParkedIn\"==\"urn:ngsi-ld:Part:c\"");
        assert_statement_snapshot!("projection-a-relationship-equality-reads-an-object-among-several", statement);
    }

    /// A Relationship object past the B-tree bound is held in `value_long_uri`,
    /// and an operand of that length is compared there.
    #[test]
    fn a_long_relationship_object_is_compared_in_its_overflow_column() {
        let object = format!("urn:ngsi-ld:Part:{}", "x".repeat(1100));
        let statement = render(&format!("\"https://example.com/isParkedIn\"==\"{object}\""));
        assert_statement_snapshot!("projection-a-long-relationship-object-is-compared-in-its-overflow-column", statement);
    }

    /// The equality operator keeps the plain comparison: clause 4.9 states the
    /// datatype rule the other way for it: "If there is no equality between the
    /// target value data type and the Query Term value data type, then it shall
    /// be considered as not matching", which is what an unknown already means.
    #[test]
    fn an_equality_is_not_negated_at_all() {
        let statement = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert_statement_snapshot!("projection-an-equality-is-not-negated-at-all", statement);
    }

    /// Clause 4.9's `unequal` over a value list: "The target value is neither
    /// identical nor equivalent to any of the list values", nor included in
    /// it when the target is an array, so the disjunction of the members'
    /// equalities, the structured value column's among them, is negated
    /// totally. The entity must still contain the target element, so the
    /// negation sits inside the one probe: negating the `EXISTS` instead would
    /// match an entity that simply lacks the attribute.
    #[test]
    fn a_negated_value_list_negates_the_whole_membership_inside_the_probe() {
        let statement = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert_statement_snapshot!("projection-a-negated-value-list-negates-the-whole-membership-inside-the-probe", statement);
    }

    /// Clause 4.9 states the same datatype rule for a range operand's negated
    /// form, so a target the interval cannot even be evaluated against is
    /// unequal to it rather than dropped.
    #[test]
    fn a_negated_range_negates_the_interval_test_totally() {
        let statement = render(&format!("{}!=10..20", quoted(SPEED)));
        assert_statement_snapshot!("projection-a-negated-range-negates-the-interval-test-totally", statement);
    }

    /// A language-selected inequality keeps its own compilation: clause 4.9
    /// requires the entity to contain the target element, the
    /// `LanguageProperty`, so a scalar-valued attribute of the same name is no
    /// match, while a `languageMap` without the key is one ("No matching value
    /// is found as the value of the specified language key").
    #[test]
    fn a_language_selected_inequality_requires_a_language_property_rather_than_the_key() {
        let statement = render("\"https://example.com/color\"[en]!=\"red\"");
        assert_statement_snapshot!(
            "projection-a-language-selected-inequality-requires-a-language-property-rather-than-the-key",
            statement
        );
    }

    #[test]
    fn no_probe_guards_against_a_stored_deletion_marker() {
        // A deleted attribute is an absent row, so there is
        // nothing to exclude and no sentinel value a client could collide with.
        let statement = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert_statement_snapshot!("projection-no-probe-guards-against-a-stored-deletion-marker", statement);
    }

    #[test]
    fn a_string_probe_reads_its_value_kind_as_true_or_unknown_only() {
        // `value_string` holds a String, a `Date` and a `Time`, so a String
        // probe names its kind; a plain `value_kind = k` would turn the NULL a
        // wrong-kind row yields into FALSE, which a negated predicate then
        // reads as a match.
        let statement = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert_statement_snapshot!("projection-a-string-probe-reads-its-value-kind-as-true-or-unknown-only", statement);
    }

    #[test]
    fn boolean_comparison_binds_native_boolean() {
        let statement = render("\"https://example.com/active\"==true");
        assert_statement_snapshot!("projection-boolean-comparison-binds-native-boolean", statement);
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let statement = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert_statement_snapshot!("projection-datetime-comparison-binds-native-timestamp", statement);
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_projection_stores_it_that_way() {
        let statement = render("\"https://example.com/d\"==2024-01-01");
        assert_statement_snapshot!("projection-date-scalar-compares-as-text-because-the-projection-stores-it-that-way", statement);
    }

    #[test]
    fn a_time_operand_binds_the_fixed_width_encoding_the_write_path_stores() {
        // A declared `Time` is stored in its fixed-width encoding, which the
        // probe binds whatever spelling clause 4.6.3 let the operand take.
        let statement = render("\"https://example.com/d\"==12:00:00.5Z");
        assert_statement_snapshot!("projection-a-time-operand-binds-the-fixed-width-encoding-the-write-path-stores", statement);
    }

    #[test]
    fn a_time_ordering_comparison_binds_the_fixed_width_encoding_too() {
        // Byte-wise order is chronological because both sides are the
        // fixed-width encoding, so the stored text is compared as it is.
        let statement = render("\"https://example.com/d\">09:30:00.5Z");
        assert_statement_snapshot!("projection-a-time-ordering-comparison-binds-the-fixed-width-encoding-too", statement);
    }

    #[test]
    fn inclusive_range_becomes_a_bounded_pair_inside_one_probe() {
        let statement = render("\"https://example.com/temperature\"==10..20");
        assert_statement_snapshot!("projection-inclusive-range-becomes-a-bounded-pair-inside-one-probe", statement);
    }

    #[test]
    fn excluded_range_requires_the_attribute_to_exist() {
        let statement = render("\"https://example.com/temperature\"!=10..20");
        assert_statement_snapshot!("projection-excluded-range-requires-the-attribute-to-exist", statement);
    }

    #[test]
    fn value_list_becomes_a_disjunction_inside_one_probe() {
        let statement = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_statement_snapshot!("projection-value-list-becomes-a-disjunction-inside-one-probe", statement);
    }

    #[test]
    fn integers_beyond_binary64_precision_use_the_numeric_projection() {
        let statement = render(&format!("{}==9007199254740993", quoted(SPEED)));
        assert_statement_snapshot!("projection-integers-beyond-binary64-precision-use-the-numeric-projection", statement);
    }

    #[test]
    fn exact_fractions_use_the_projection() {
        let statement = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert_statement_snapshot!("projection-exact-fractions-use-the-projection", statement);
    }

    #[test]
    fn pattern_match_uses_posix_regex_on_the_string_column() {
        let statement = render("\"https://example.com/name\"~=\"^A.*\"");
        assert_statement_snapshot!("projection-pattern-match-uses-posix-regex-on-the-string-column", statement);
    }

    #[test]
    fn negated_pattern_sits_inside_the_probe_so_missing_attributes_do_not_match() {
        let statement = render("\"https://example.com/name\"!~=\"^A.*\"");
        assert_statement_snapshot!("projection-negated-pattern-sits-inside-the-probe-so-missing-attributes-do-not-match", statement);
    }

    /// Clause 4.9: with no target language specified, a `LanguageProperty`'s
    /// target value is "a value from any of the key-value pairs held within the
    /// languageMap", and a `ListProperty`'s is "the valueList array", both of
    /// which the write path stores in `value_object`, so an equality Query
    /// Term has to reach that column and not only the typed ones.
    #[test]
    fn an_unqualified_equality_reaches_the_structured_value_column() {
        let statement = render("\"https://example.com/color\"==\"red\"");
        assert_statement_snapshot!("projection-an-unqualified-equality-reaches-the-structured-value-column", statement);
    }

    /// Clause 4.9: "the target object of a `ListRelationship` target element is
    /// the array of objects (URIs) associated with it", and a URI Query Term
    /// matches when it "is included in the target value". The objects are
    /// projected as bare array elements, which is what the unwrapped
    /// containment shape reaches.
    #[test]
    fn a_uri_query_term_reaches_an_object_list_through_the_bare_element_shape() {
        let statement = render("\"https://example.com/color\"==\"urn:ngsi-ld:Part:1\"");
        assert_statement_snapshot!("projection-a-uri-query-term-reaches-an-object-list-through-the-bare-element-shape", statement);
    }

    /// Clause 4.9: "the target value is defined as the Value associated to the
    /// matching key-value pair held within the languageMap ... where the key
    /// matches the target language". The row decides whether `attr[en]` means
    /// that or a compound value member, so both are compiled, as one test of
    /// the attribute's rows.
    #[test]
    fn a_target_language_compiles_a_language_map_reading_beside_the_member_reading() {
        let statement = render("\"https://example.com/color\"[en]==\"red\"");
        assert_statement_snapshot!(
            "projection-a-target-language-compiles-a-language-map-reading-beside-the-member-reading",
            statement
        );
    }

    /// A language-selected path addresses a `languageMap` key. A scalar-valued
    /// attribute of the same name has no such key, so clause 4.9's target
    /// element does not exist for it and neither polarity may consult its typed
    /// column: `color[en]!=5` must not select a plain numeric Property.
    #[test]
    fn a_target_language_never_consults_the_typed_scalar_columns() {
        for (index, expression) in ["\"https://example.com/color\"[en]==5", "\"https://example.com/color\"[en]!=5"]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-a-target-language-never-consults-the-typed-scalar-columns-{}", index + 1),
                render(expression)
            );
        }
    }

    /// Clause 4.9's `unequal` over a structured target: "Is not included in the
    /// target value, if the latter is an array", and over a scalar one: "If the
    /// data type of the target value and the data type of the Query Term value
    /// are different, then they shall be considered unequal". One total negation
    /// of the whole `equal` condition states both, and the enclosing `EXISTS` is
    /// what still requires the target element.
    #[test]
    fn an_unqualified_inequality_negates_every_reading_of_the_target_value_at_once() {
        let statement = render("\"https://example.com/color\"!=\"red\"");
        assert_statement_snapshot!(
            "projection-an-unqualified-inequality-negates-every-reading-of-the-target-value-at-once",
            statement
        );
    }

    /// Clause 4.9 admits a value list against every target: "The target value
    /// includes any of the Query Term values, if the target value is an array".
    #[test]
    fn a_value_list_reaches_the_structured_value_column_once_per_member() {
        let statement = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_statement_snapshot!("projection-a-value-list-reaches-the-structured-value-column-once-per-member", statement);
    }

    /// Clause 4.9 requires a matching Entity to contain the target element,
    /// which for a language-selected path is the key-value pair itself.
    #[test]
    fn a_bare_existence_of_a_target_language_probes_the_language_key() {
        let statement = render("\"https://example.com/color\"[en]");
        assert_statement_snapshot!("projection-a-bare-existence-of-a-target-language-probes-the-language-key", statement);
    }

    /// Clause 4.9's ordering, pattern and range operators are not containment
    /// tests, so nothing probes the structured column by containment under
    /// them. A `LanguageProperty`'s strings are each a String target value with
    /// no target language, so a String operand reads them one by one, only on
    /// a row written as a `LanguageProperty`, never a `ListProperty`'s array.
    #[test]
    fn ordering_pattern_and_string_range_read_a_language_map_only_on_a_language_property() {
        for (index, expression) in [
            "\"https://example.com/color\">\"red\"",
            "\"https://example.com/color\"~=\"re.*\"",
            "\"https://example.com/color\"!~=\"re.*\"",
            "\"https://example.com/color\"==\"a\"..\"m\"",
            "\"https://example.com/color\"!=\"a\"..\"m\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!(
                    "projection-ordering-pattern-and-string-range-read-a-language-map-only-on-a-language-property-{}",
                    index + 1
                ),
                render(expression)
            );
        }
    }

    /// A number or a boolean is a datatype no string carries, so its ordering
    /// and range terms read the typed column alone and keep its index.
    #[test]
    fn a_numeric_ordering_or_range_reads_only_its_typed_column() {
        for (index, expression) in [
            "\"https://example.com/temperature\">20",
            "\"https://example.com/temperature\"==10..20",
            "\"https://example.com/temperature\"!=10..20",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-a-numeric-ordering-or-range-reads-only-its-typed-column-{}", index + 1),
                render(expression)
            );
        }
    }

    /// Clause 4.9: an excluded range holds over a language map none of whose
    /// strings lies in the interval, which is the total negation of the one
    /// interval test that reads the typed column and the strings together.
    #[test]
    fn an_excluded_string_range_negates_the_strings_with_the_typed_column() {
        let statement = render("\"https://example.com/color\"!=\"a\"..\"m\"");
        assert_statement_snapshot!("projection-an-excluded-string-range-negates-the-strings-with-the-typed-column", statement);
    }

    /// Clause 4.9: with a target language the target value is a String of that
    /// language, so the ordering, pattern and range operators read its strings
    /// on the projection, disjoined with the compound-member reading.
    #[test]
    fn a_target_language_reads_that_languages_strings_under_every_operator() {
        for (index, expression) in [
            "\"https://example.com/color\"[en]>\"m\"",
            "\"https://example.com/color\"[en]~=\"^r\"",
            "\"https://example.com/color\"[en]!~=\"^r\"",
            "\"https://example.com/color\"[en]==\"a\"..\"m\"",
            "\"https://example.com/color\"[en]!=\"a\"..\"m\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-a-target-language-reads-that-languages-strings-under-every-operator-{}", index + 1),
                render(expression)
            );
        }
    }

    /// The complementary forms require the `LanguageProperty`, not the target
    /// language, and deny every string of that language (clause 4.9: "No
    /// matching value is found as the value of the specified language key").
    #[test]
    fn a_target_language_complement_requires_the_language_property_rather_than_the_language() {
        for (index, expression) in ["\"https://example.com/color\"[en]!~=\"^r\"", "\"https://example.com/color\"[en]!=\"a\"..\"m\""]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!(
                    "projection-a-target-language-complement-requires-the-language-property-rather-than-the-language-{}",
                    index + 1
                ),
                render(expression)
            );
        }
    }

    /// Clause 4.9's target element is the last name of the dotted path, so a
    /// `LanguageProperty` sub-attribute is read through the `languageMap` its
    /// element holds, beside the reading of a compound member.
    #[test]
    fn a_sub_attribute_is_read_through_the_language_map_of_its_element() {
        for (index, expression) in [
            "\"https://example.com/address\".\"https://example.com/label\"[en]==\"Front\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]!=\"Front\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]",
            "\"https://example.com/address\".\"https://example.com/label\"[en]>\"F\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]~=\"^F\"",
            "\"https://example.com/address\".\"https://example.com/label\"==\"Front\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-a-sub-attribute-is-read-through-the-language-map-of-its-element-{}", index + 1),
                render(expression)
            );
        }
    }

    /// A nested compound path below a sub-attribute has no language reading.
    #[test]
    fn a_nested_compound_path_below_a_sub_attribute_reads_no_language_map() {
        let statement = render("\"https://example.com/address\".\"https://example.com/label\"[a.b]==\"x\"");
        assert_statement_snapshot!("projection-a-nested-compound-path-below-a-sub-attribute-reads-no-language-map", statement);
    }

    #[test]
    fn unknown_attribute_iri_compiles_to_a_constant_false() {
        let statement = render_with(
            &format!("\"{}\">1", "https://example.com/never-written"),
            &names(),
            &EntityRowRefs::unaliased(None),
        );
        assert_statement_snapshot!("projection-unknown-attribute-iri-compiles-to-a-constant-false", statement);
    }

    #[test]
    fn bare_existence_is_the_presence_of_a_projection_row() {
        // A deleted attribute leaves no row, so presence needs no second probe
        // and no tombstone test.
        let statement = render(&quoted(SPEED));
        assert_statement_snapshot!("projection-bare-existence-is-the-presence-of-a-projection-row", statement);
    }

    #[test]
    fn unknown_attribute_existence_compiles_to_a_constant_false() {
        let statement = render("\"https://example.com/never-written\"");
        assert_statement_snapshot!("projection-unknown-attribute-existence-compiles-to-a-constant-false", statement);
    }

    /// An unknown attribute names nothing a stored instance holds, so a
    /// conjunction holding it is constant `FALSE` for the whole filter, and a
    /// disjunction is its other arms.
    #[test]
    fn an_unknown_attribute_folds_its_conjunction_and_drops_from_its_disjunction() {
        for (index, q) in [
            format!("{}>50;\"https://example.com/never-written\"==1", quoted(SPEED)),
            format!("{}>50|\"https://example.com/never-written\"==1", quoted(SPEED)),
        ]
        .iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!(
                    "projection-an-unknown-attribute-folds-its-conjunction-and-drops-from-its-disjunction-{}",
                    index + 1
                ),
                render(q)
            );
        }
    }

    #[test]
    fn and_or_nesting_preserves_grouping() {
        let statement = render(&format!(
            "({}>1;\"https://example.com/b\"<2)|\"https://example.com/c\"==\"x\"",
            quoted("https://example.com/a")
        ));
        assert_statement_snapshot!("projection-and-or-nesting-preserves-grouping", statement);
    }

    /// Some instance matching one Query Term or some instance matching another
    /// is some instance matching either (clause 4.9), so a disjunction is one
    /// probe whose arms each keep their own attribute key.
    #[test]
    fn a_disjunction_of_terms_shares_one_probe() {
        for (index, expression) in [
            "\"https://example.com/color\"==\"red\"|\"https://example.com/color\"==\"blue\"",
            "\"https://example.com/color\"==\"red\"|\"https://example.com/name\"!=\"x\"|\"https://example.com/a\">1",
            "\"https://example.com/color\"|\"https://example.com/name\"~=\"^A\"",
            "(\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"x\")|\"https://example.com/a\"==1",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(format!("projection-a-disjunction-of-terms-shares-one-probe-{}", index + 1), render(expression));
        }
    }

    /// The arms of a shared probe bind in the order the terms were written, each
    /// attribute key ahead of its own operands.
    #[test]
    fn a_shared_probe_binds_each_attribute_key_ahead_of_its_operands() {
        let statement = render("\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"x\"");
        assert_statement_snapshot!("projection-a-shared-probe-binds-each-attribute-key-ahead-of-its-operands", statement);
    }

    /// A linked-entity hop reads other rows than the scan does, so it stands
    /// beside the shared probe, which a compound member reading joins: it reads
    /// the `value_object` of a row of its attribute.
    #[test]
    fn hops_stand_beside_the_shared_probe_and_member_readings_join_it() {
        let statement = render(&format!(
            "{}{{{}}}==\"x\"|\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"y\"|\"https://example.com/address\"[street]>\"a\"",
            quoted(IS_PARKED_IN),
            quoted(CATEGORY)
        ));
        assert_statement_snapshot!("projection-hops-stand-beside-the-shared-probe-and-member-readings-join-it", statement);
    }

    /// A range on a sub-attribute has no key to read, so its probe walks: it
    /// carries an `OFFSET`, which keeps it a per-row probe in page order, and
    /// rechecks the element inline.
    #[test]
    fn a_sub_attribute_range_walks() {
        let statement = render("\"https://example.com/address\".\"https://example.com/label\">0.5");
        assert_statement_snapshot!("projection-a-sub-attribute-range-walks", statement);
    }

    /// No trigram index covers an element's strings, so a pattern on a
    /// sub-attribute walks and reads the element's one string field.
    #[test]
    fn a_sub_attribute_pattern_walks_without_a_trigram_filter() {
        let statement = render("\"https://example.com/address\".\"https://example.com/label\"~=\"^x\"");
        assert_statement_snapshot!("projection-a-sub-attribute-pattern-walks-without-a-trigram-filter", statement);
    }

    /// A top-level pattern is matched whole: PostgreSQL extracts the trigrams
    /// `idx_ca_value_string_trgm` filters on from the pattern itself, anchors
    /// included, and newline-sensitive mode is never set.
    #[test]
    fn a_top_level_pattern_keeps_its_anchors_and_never_sets_newline_mode() {
        let statement = render("\"https://example.com/color\"~=\"^x-[0-9]$\"");
        assert_statement_snapshot!("projection-a-top-level-pattern-keeps-its-anchors-and-never-sets-newline-mode", statement);
    }

    /// A member of a sub-attribute's compound value is read out of the
    /// element's structured value, which no key covers, so the term walks: its
    /// `value_object`, or the proof a data-integrity seal holds in its own
    /// fields.
    #[test]
    fn a_value_object_sub_attribute_walks() {
        let statement = render("\"https://example.com/address\".\"https://example.com/label\"[c]==\"x\"");
        assert_statement_snapshot!("projection-a-value-object-sub-attribute-walks", statement);
    }

    /// A path ending on a member of the attribute reads the field of its row,
    /// with no element chain at all.
    #[test]
    fn a_member_of_the_attribute_is_read_from_its_row() {
        let statement = render(&format!("{}.\"https://uri.etsi.org/ngsi-ld/observedAt\">2024-01-01T00:00:00Z", quoted(SPEED)));
        assert_statement_snapshot!("projection-a-member-of-the-attribute-is-read-from-its-row", statement);
    }

    /// Clause 4.9 evaluates a trailing group against the target value, which
    /// for an instance member is a String or a `DateTime` with no member: the
    /// target element does not exist, so the term holds for no Entity, under
    /// `!=` too.
    #[test]
    fn a_member_before_a_trailing_group_holds_for_no_entity() {
        let statement = render(&format!("{}.\"https://uri.etsi.org/ngsi-ld/observedAt\"[x]", quoted(SPEED)));
        assert_statement_snapshot!("projection-a-member-before-a-trailing-group-holds-for-no-entity", statement);
        let unequal = render(&format!(
            "{}.\"https://uri.etsi.org/ngsi-ld/observedAt\"[x]!=2024-01-01T00:00:00Z",
            quoted(SPEED)
        ));
        assert_statement_snapshot!("projection-a-member-before-a-trailing-group-holds-for-no-entity", unequal);
    }

    /// Two terms one instance must pass together are not two terms two
    /// instances pass separately, so a conjunction keeps a probe per term.
    #[test]
    fn a_conjunction_keeps_one_probe_per_term() {
        let statement = render("\"https://example.com/color\"==\"red\";\"https://example.com/name\"==\"x\"");
        assert_statement_snapshot!("projection-a-conjunction-keeps-one-probe-per-term", statement);
    }

    #[test]
    fn a_correlated_statement_probes_against_the_entity_key_of_its_own_alias() {
        let statement = render_with(
            &format!("{}>50", quoted(SPEED)),
            &names(),
            &EntityRowRefs::aliased(None, Alias::new("ce"), Alias::new("cs")),
        );
        assert_statement_snapshot!("projection-a-correlated-statement-probes-against-the-entity-key-of-its-own-alias", statement);
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_surrogate_key() {
        let statement = render(&format!("{}>50", quoted(SPEED)));
        assert_statement_snapshot!("projection-entity-queries-correlate-on-the-entities-surrogate-key", statement);
    }

    const IS_PARKED_IN: &str = "https://example.com/isParkedIn";
    const CATEGORY: &str = "https://example.com/category";

    #[test]
    fn a_single_hop_probes_the_relationship_and_joins_the_entity_it_names() {
        let statement = render(&format!("{}{{{}}}==\"commercial\"", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert_statement_snapshot!("projection-a-single-hop-probes-the-relationship-and-joins-the-entity-it-names", statement);
    }

    #[test]
    fn a_hop_guard_tests_the_linked_entitys_types_for_any_of_its_alternatives() {
        // Clause 4.9 lists the guard's types as alternatives, so one membership
        // test against `entities.entity_type_name_keys` per resolved type, disjoined.
        // The guard renders as a bare `a OR b`, and without parentheses around it
        // the second disjunct escapes the correlation, the attribute-name test and
        // the tombstone guard, so the probe would match whenever any entity in the
        // tenant carried the second type.
        let statement = render(&format!(
            "{}{{{},{}:{}}}==\"x\"",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Store"),
            quoted(CATEGORY)
        ));
        assert_statement_snapshot!("projection-a-hop-guard-tests-the-linked-entitys-types-for-any-of-its-alternatives", statement);
    }

    /// A guard type the dictionary never recorded was never carried by any
    /// entity, so the guard admits the resolved one alone.
    #[test]
    fn an_unresolved_guard_type_is_no_alternative_of_the_guard() {
        let statement = render(&format!(
            "{}{{{},{}:{}}}==\"x\"",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Warehouse"),
            quoted(CATEGORY)
        ));
        assert_statement_snapshot!("projection-an-unresolved-guard-type-is-no-alternative-of-the-guard", statement);
    }

    #[test]
    fn an_unguarded_hop_emits_no_type_test() {
        let statement = render(&format!("{}{{{}}}==\"commercial\"", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert_statement_snapshot!("projection-an-unguarded-hop-emits-no-type-test", statement);
    }

    #[test]
    fn nested_hops_number_their_aliases_by_depth() {
        let statement = render(&format!(
            "{}{{{}{{{}}}}}==\"north\"",
            quoted("https://example.com/outer"),
            quoted("https://example.com/inner"),
            quoted("https://example.com/zone")
        ));
        assert_statement_snapshot!("projection-nested-hops-number-their-aliases-by-depth", statement);
    }

    #[test]
    fn a_bare_existence_through_a_hop_probes_the_rows_of_the_linked_entity() {
        let statement = render(&format!("{}{{{}}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert_statement_snapshot!("projection-a-bare-existence-through-a-hop-probes-the-rows-of-the-linked-entity", statement);
    }

    #[test]
    fn an_unresolved_relationship_name_makes_the_traversal_unsatisfiable() {
        // No `attribute_names` row means no instance of the Relationship was
        // ever written, so no entity is reachable through it.
        let statement = render(&format!("{}{{{}}}==\"x\"", quoted("https://example.com/never-written"), quoted(CATEGORY)));
        assert_statement_snapshot!("projection-an-unresolved-relationship-name-makes-the-traversal-unsatisfiable", statement);
    }

    /// Annex C.5.13.2: a tag no member name spells has the languageMap reading
    /// alone, its key compared case-insensitively (RFC 5646 section 2.1.1) and
    /// never by JSONB containment, which compares bytes.
    #[test]
    fn a_hyphenated_tag_compiles_the_language_map_reading_alone() {
        let statement = render("\"https://example.com/color\"[en-GB]==\"red\"");
        assert_statement_snapshot!("projection-a-hyphenated-tag-compiles-the-language-map-reading-alone", statement);
    }

    /// A languageMap holds strings only; clause 4.9 makes a datatype difference
    /// a non-match under `==` and an inequality under `!=`.
    #[test]
    fn a_number_under_a_target_language_matches_no_string() {
        assert_statement_snapshot!(
            "projection-a-number-under-a-target-language-matches-no-string-1",
            render("\"https://example.com/color\"[en-GB]==5")
        );
        assert_statement_snapshot!(
            "projection-a-number-under-a-target-language-matches-no-string-2",
            render("\"https://example.com/color\"[en-GB]!=5")
        );
    }

    /// RFC 4647 section 3.3.2 at the `language-ranges` level:
    /// the key is matched against the range's pattern, bound as a parameter.
    #[test]
    fn extended_filtering_matches_every_target_language_by_its_pattern() {
        for (index, expression) in ["\"https://example.com/color\"[en]==\"red\"", "\"https://example.com/color\"[de-*-DE]==\"red\""]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-extended-filtering-matches-every-target-language-by-its-pattern-{}", index + 1),
                render_under(expression, &names(), &EntityRowRefs::unaliased(None), LanguageMatching::ExtendedFiltering)
            );
        }
    }

    /// A stored wildcard range keeps filtering under tag equality: it has no
    /// equality reading.
    #[test]
    fn a_range_filters_under_tag_equality_too() {
        let statement = render("\"https://example.com/color\"[*-GB]~=\"^L\"");
        assert_statement_snapshot!("projection-a-range-filters-under-tag-equality-too", statement);
    }

    fn select_where(condition: Condition) -> SelectStatement {
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
    }

    /// The statement answers the term it leaves out, so a conjunct asking the
    /// same is left out with it, each as no constraint, and one inside a
    /// disjunction, which the statement does not answer, stays.
    #[test]
    fn compile_except_leaves_out_every_conjunct_equal_to_the_answered_term() {
        let rows = EntityRowRefs::unaliased(None);
        let scan = InstanceScan::Current(&rows);
        let answered = term_of("\"https://example.com/a\"==1");
        for (index, q) in [
            "\"https://example.com/a\"==1;\"https://example.com/b\"==2;\"https://example.com/a\"==1",
            "\"https://example.com/b\"==2;(\"https://example.com/a\"==1|\"https://example.com/c\"==3)",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("projection-compile-except-leaves-out-every-conjunct-equal-to-the-answered-term-{}", index + 1),
                select_where(compile_except(&reading_of(q), &answered, &scan, &names()))
            );
        }
    }
}
