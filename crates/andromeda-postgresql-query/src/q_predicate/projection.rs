//! `q` predicates compiled to `EXISTS` probes against the stored attribute
//! instances an [`InstanceScan`] names: the `current_attributes` rows for a
//! Query Entities `q`, the in-window `attribute_history` rows for a temporal
//! one.
//!
//! This module routes each Query Term by what its path addresses (see
//! [`local_target`]) to the readings that can hold its target value, and
//! compiles each reading with the comparison of its operator
//! (`equality_term`, `ordering_term`, `pattern_term`, and existence here). It
//! descends linked-entity hops first.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::equality_value,
        instance_row::InstanceRow,
        instance_scan::InstanceScan,
        instance_test::{CompiledTerm, Disjunction, InstanceTest},
        json_value_term::{JsonTarget, json_equality, json_ordering, json_pattern},
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
        local_target::{LocalTarget, local_target},
        ordering_term::ordering_value,
        pattern_term::pattern_value,
        structured_target::{StructuredTarget, carries_target_language},
        sub_attribute::element_path::ElementPath,
        traversal::LinkedHop,
        value_column::ValueColumn,
        value_object_member::on_some_member,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{
    expression::Expr,
    path::{AttrPath, QueryPath},
    predicate::{PatternPolarity, Predicate},
};
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::Condition;
use std::ptr;

/// Compile a parsed `q` expression into a sea-query [`Condition`].
///
/// Compilation cannot fail: an attribute or entity type name the dictionary
/// does not hold compiles to a constant `FALSE` rather than erroring, and every
/// operand is bound as a parameter rather than converted, so every expression
/// clause 4.9 admits has a condition.
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
pub fn compile(expr: &Expr, scan: &InstanceScan<'_>, names: &QueryNames) -> Condition {
    compile_with(expr, scan, names, None)
}

/// [`compile`], with `answered` (a term of `expr`'s top-level conjunction the
/// statement answers on its own) compiled as no constraint.
#[must_use]
pub fn compile_except(expr: &Expr, answered: &Predicate, scan: &InstanceScan<'_>, names: &QueryNames) -> Condition {
    compile_with(expr, scan, names, Some(answered))
}

/// Compile `expr`, leaving out the one term `answered` names.
fn compile_with(expr: &Expr, scan: &InstanceScan<'_>, names: &QueryNames, answered: Option<&Predicate>) -> Condition {
    match expr {
        Expr::And(terms) => terms
            .iter()
            .fold(Condition::all(), |cond, term| cond.add(compile_with(term, scan, names, answered))),
        Expr::Or(terms) => disjunction(terms, Disjunction::default(), scan, names).into_condition(scan),
        Expr::Pred(pred) if answered.is_some_and(|answered| ptr::eq(answered, pred)) => Condition::all(),
        Expr::Pred(pred) => compile_predicate(pred, scan, names).into_condition(scan),
    }
}

/// Gather the terms of a disjunction, and of every disjunction nested directly
/// in it, into `gathered`.
fn disjunction(terms: &[Expr], gathered: Disjunction, scan: &InstanceScan<'_>, names: &QueryNames) -> Disjunction {
    terms.iter().fold(gathered, |gathered, term| match term {
        Expr::Or(inner) => disjunction(inner, gathered, scan, names),
        Expr::And(_) => gathered.with(CompiledTerm::Standalone(compile(term, scan, names))),
        Expr::Pred(pred) => gathered.with(compile_predicate(pred, scan, names)),
    })
}

/// Compile one leaf predicate, descending any linked-entity hops its axis
/// carries before compiling the comparison against the entity it lands on.
fn compile_predicate(pred: &Predicate, scan: &InstanceScan<'_>, names: &QueryNames) -> CompiledTerm {
    compile_along_path(pred, pred.path(), scan, names, 0)
}

/// Compile a predicate against the entity reached by the remaining axis.
///
/// `depth` is the number of hops already descended; it suffixes the aliases so
/// a nested traversal's probes cannot shadow an enclosing one's.
///
/// A hop lands on the linked Entity as it stands: the instances `scan` reads
/// decide which entities the Relationship named, and the linked entity is then
/// asked about its current state, because a window selects instances of the
/// entity being queried and says nothing of the entities it points at.
fn compile_along_path(pred: &Predicate, path: &QueryPath, scan: &InstanceScan<'_>, names: &QueryNames, depth: usize) -> CompiledTerm {
    match path {
        QueryPath::Local(local) => compile_local(pred, local, scan, names),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let Some(relationship_id) = names.attributes.get(relationship) else {
                // No `attribute_names` row means no instance of the
                // Relationship was ever written, so no entity can be reached
                // through it.
                return unsatisfiable_term();
            };
            let hop = LinkedHop::at_depth(depth + 1);
            let inner_rows = hop.inner_rows(scan.schema().cloned());
            let inner_condition = linked_condition(pred, inner, &inner_rows, names, depth + 1);
            CompiledTerm::Standalone(hop.probe(relationship_id, guard_types, &names.types, scan, inner_condition))
        }
    }
}

/// What `pred` asks, along the rest `path` of its axis, of the linked entity
/// `rows` names, reached `depth` hops into a traversal.
pub(crate) fn linked_condition(pred: &Predicate, path: &QueryPath, rows: &EntityRowRefs, names: &QueryNames, depth: usize) -> Condition {
    let scan = InstanceScan::Current(rows);
    compile_along_path(pred, path, &scan, names, depth).into_condition(&scan)
}

/// Where an instance holds the value a term with no target language reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// The instance's own value (clause 4.9 target value with no trailing
    /// path), a `languageMap` read in every language.
    OwnValue,
    /// The members of a compound value the trailing path addresses.
    Members,
}

/// The reading the trailing group of a path gives its target value.
///
/// One trailing member has a second reading beside the compound member, the
/// target language of a `LanguageProperty` (see [`target_language`]), and the
/// stored instance decides which applies. Neither can match the other's
/// instances (a `languageMap` has no IRI-keyed members, a compound value no
/// `@language`), so both are compiled and disjoined.
const fn reading(target: LocalTarget<'_>) -> Reading {
    match target {
        LocalTarget::OwnValue | LocalTarget::SubAttribute(Some(LanguageSelection::AnyLanguage)) => Reading::OwnValue,
        LocalTarget::TrailingMember(_) | LocalTarget::NestedMember | LocalTarget::SubAttribute(Some(LanguageSelection::TargetLanguage(_)) | None) => {
            Reading::Members
        }
    }
}

/// The target language of a one-member trailing group, if `target` has one.
const fn target_language(target: LocalTarget<'_>) -> Option<&AttributeName> {
    match target {
        LocalTarget::TrailingMember(member) | LocalTarget::SubAttribute(Some(LanguageSelection::TargetLanguage(member))) => Some(member),
        LocalTarget::OwnValue | LocalTarget::NestedMember | LocalTarget::SubAttribute(Some(LanguageSelection::AnyLanguage) | None) => None,
    }
}

/// Compile a term over the instances `scan` reads.
///
/// The target element is held by a row of the attribute, or by an element of
/// the row's `sub_attributes` for a sub-attribute path; a current row and a
/// history row hold both alike. A row's test joins the probe of its attribute,
/// which a disjunction shares; an element's test is a probe of its own (see
/// [`InstanceScan::exists_in_tree`]).
fn compile_local(pred: &Predicate, path: &AttrPath, scan: &InstanceScan<'_>, names: &QueryNames) -> CompiledTerm {
    let Some(element) = ElementPath::resolve(path, scan.row(), &names.attributes) else {
        return unsatisfiable_term();
    };
    let target = local_target(path);
    let holder = element.holder();
    let value = if let Some(field) = element.field {
        field.condition(&field.of_instance(holder), pred)
    } else {
        let own = match reading(target) {
            Reading::OwnValue => own_value(pred, holder),
            Reading::Members => on_some_member(path, holder, |member| json_reading(pred, member)),
        };
        match target_language(target) {
            Some(language) => Condition::any().add(language_reading(pred, language, holder)).add(own),
            None => own,
        }
    };
    if element.chain.is_empty() {
        return CompiledTerm::Instances(InstanceTest {
            attribute: element.attribute,
            value,
        });
    }
    CompiledTerm::Standalone(scan.exists_in_tree(element.attribute, element.condition(value)))
}

/// A term against the instance's own value.
fn own_value(pred: &Predicate, row: InstanceRow) -> Condition {
    match pred {
        // An instance *is* the attribute: an attribute deleted by clause
        // 5.6.16 leaves no row and no element behind, so presence needs no
        // further guard.
        Predicate::Exists(_) => Condition::all(),
        Predicate::Equality { membership, rhs, .. } => equality_value(StructuredTarget::AnyValue, *membership, rhs, row),
        Predicate::Ordering { op, rhs, .. } => ordering_value(pred, *op, rhs, row),
        Predicate::MatchPattern { pattern, .. } => pattern_value(pred, pattern, PatternPolarity::Match, row),
        Predicate::NotMatchPattern { pattern, .. } => pattern_value(pred, pattern, PatternPolarity::NotMatch, row),
    }
}

/// A term against the strings a `languageMap` holds under one target
/// language (clause 4.9).
///
/// Existence asks for the matching key-value pair, which is the target element
/// of a language-selected path.
fn language_reading(pred: &Predicate, language: &AttributeName, row: InstanceRow) -> Condition {
    match pred {
        Predicate::Exists(_) => carries_target_language(&row.value(ValueColumn::Object), language),
        Predicate::Equality { membership, rhs, .. } => equality_value(StructuredTarget::TargetLanguage(language), *membership, rhs, row),
        Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => {
            Condition::all().add(language_term(pred, &LanguageMap::of_instance(row), LanguageSelection::TargetLanguage(language)))
        }
    }
}

/// A term against one member of a compound value.
fn json_reading(pred: &Predicate, member: &JsonTarget) -> Condition {
    match pred {
        Predicate::Exists(_) => Condition::all().add(member.presence.clone()), // clone: the member is read by every test
        Predicate::Equality { membership, rhs, .. } => json_equality(member, *membership, rhs),
        Predicate::Ordering { op, rhs, .. } => json_ordering(member, *op, rhs),
        Predicate::MatchPattern { pattern, .. } => json_pattern(member, pattern, PatternPolarity::Match),
        Predicate::NotMatchPattern { pattern, .. } => json_pattern(member, pattern, PatternPolarity::NotMatch),
    }
}

/// A predicate that no row can satisfy.
pub(crate) fn unsatisfiable() -> Condition {
    Condition::all().add(false_literal())
}

/// A Query Term that no row can satisfy.
pub(crate) fn unsatisfiable_term() -> CompiledTerm {
    CompiledTerm::Standalone(unsatisfiable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_row_refs::EntityRowRefs,
        entity_type_name_key::EntityTypeNameKey,
        temporal_operand::STORED_TIME_TEMPLATE,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::parser;
    use andromeda_postgresql_schema::{current::Entities, string_value_column::INDEXED_STRING_BYTE_LIMIT};
    use sea_query::{Alias, PostgresQueryBuilder, Query, Value};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::from_pairs([
                ("https://example.com/Building".parse().expect("valid type name"), EntityTypeNameKey::from(31)),
                ("https://example.com/Store".parse().expect("valid type name"), EntityTypeNameKey::from(32)),
            ]),
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

    /// Render a compiled condition the way a real query would, so the assertions
    /// see the final placeholder numbering.
    fn render(q: &str) -> (String, Vec<Value>) {
        render_with(q, &names(), &EntityRowRefs::unaliased(None))
    }

    fn render_with(q: &str, names: &QueryNames, rows: &EntityRowRefs) -> (String, Vec<Value>) {
        let expr = parser::parse(q).expect("parse q");
        let condition = compile(&expr, &InstanceScan::Current(rows), names);
        let (sql, values) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        (sql, values.0)
    }

    fn quoted(iri: &str) -> String {
        format!("\"{iri}\"")
    }

    #[test]
    fn number_comparison_probes_typed_number_column() {
        let (sql, binds) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"current_attributes\""), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" >"), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(7))), "attribute id binds as int4: {binds:?}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "50"),
            "numeric literal binds as NUMERIC: {binds:?}"
        );
    }

    #[test]
    fn a_number_comparison_binds_numeric_without_a_double_conversion() {
        let (sql, binds) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert!(sql.contains("\"ca\".\"value_number\" > $2"), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "0.100000000000000005"),
            "{binds:?}"
        );
    }

    #[test]
    fn a_pattern_does_not_reach_relationship_objects() {
        // Clause 4.9 requires a Relationship operand to non-match every operator
        // other than equality and inequality, and the pattern operators are
        // defined over String targets. `value_uri` holds Relationship objects and
        // nothing else, so a pattern must not probe it.
        let (sql, _) = render("\"https://example.com/brandName\"~=\"Merc.*\"");
        assert!(sql.contains("\"ca\".\"value_string\" ~"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_long_text\" ~"), "a pattern reads both string columns: {sql}");
        assert!(!sql.contains("value_uri"), "a pattern must not test relationship objects: {sql}");
    }

    #[test]
    fn a_negated_pattern_does_not_reach_relationship_objects_either() {
        let (sql, _) = render("\"https://example.com/brandName\"!~=\"Merc.*\"");
        assert!(sql.contains("\"ca\".\"value_string\" !~"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_long_text\" !~"), "{sql}");
        assert!(!sql.contains("value_uri"), "{sql}");
    }

    #[test]
    fn equality_covers_property_values_and_relationship_objects() {
        for expression in [
            "\"https://example.com/brandName\"==\"Mercedes\"",
            "\"https://example.com/brandName\"!=\"Mercedes\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains("\"ca\".\"value_string\" "), "{expression}: {sql}");
            assert!(
                sql.contains("\"ca\".\"value_uri\" "),
                "{expression}: relationship objects are legal equality targets: {sql}"
            );
        }
    }

    #[test]
    fn an_ordering_comparison_does_not_reach_relationship_objects() {
        // Clause 4.9: a Relationship operand shall non-match every operator other
        // than equality and inequality. A URI carries no order a `<` could mean.
        for expression in [
            "\"https://example.com/brandName\">\"Mercedes\"",
            "\"https://example.com/brandName\">=\"Mercedes\"",
            "\"https://example.com/brandName\"<\"Mercedes\"",
            "\"https://example.com/brandName\"<=\"Mercedes\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains("(\"ca\".\"value_string\") COLLATE \"und-x-icu\""), "{expression}: {sql}");
            assert!(!sql.contains("value_uri"), "{expression}: {sql}");
        }
    }

    #[test]
    fn a_string_range_bound_keeps_relationship_objects_out_of_its_ordering_halves() {
        // A range compiles to two ordering comparisons, so it inherits the
        // clause 4.9 gating rather than restating it.
        let (sql, _) = render("\"https://example.com/brandName\"==\"a\"..\"z\"");
        assert!(!sql.contains("value_uri"), "{sql}");
    }

    #[test]
    fn a_value_list_still_covers_relationship_objects_in_both_polarities() {
        // Every arm of a value list is an equality comparison (clause 4.9
        // Table 4.9-1), so each one may match a Relationship's object.
        for expression in [
            "\"https://example.com/color\"==\"red\",\"blue\"",
            "\"https://example.com/color\"!=\"red\",\"blue\"",
        ] {
            let (sql, _) = render(expression);
            assert_eq!(sql.matches("value_uri").count(), 2, "{expression}: one per list member: {sql}");
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
        let (sql, _) = render("\"https://example.com/brandName\"!=\"Mercedes\"");
        assert!(sql.contains("NOT COALESCE("), "the negation reads an unknown as a non-match: {sql}");
        assert!(sql.contains("\"ca\".\"value_string\" = "), "the inner condition is the equality one: {sql}");
        assert!(sql.contains("\"ca\".\"value_uri\" = "), "{sql}");
        assert!(sql.contains("\"ca\".\"value_object\" @> "), "{sql}");
        assert!(!sql.contains(" != "), "no per-column inequality survives: {sql}");
    }

    /// A long operand's equality reads only the overflow column, and its
    /// inequality is still the total negation of that equality (clause 4.9
    /// `unequal`), so a short stored value (whose overflow arm is unknown)
    /// is unequal to it rather than dropped.
    #[test]
    fn a_negated_long_string_equality_is_the_total_negation_of_the_overflow_comparison() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let (sql, _) = render(&format!("\"https://example.com/name\"!=\"{long}\""));
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(sql.contains("\"ca\".\"value_long_text\" = "), "{sql}");
        assert!(!sql.contains("\"ca\".\"value_string\" = "), "{sql}");
    }

    /// A string range is two ordering comparisons, and each reads both string
    /// columns under the ICU root collation (clause 4.9 recommends the UCA).
    #[test]
    fn a_string_range_reads_both_string_columns_on_each_bound() {
        let (sql, _) = render("\"https://example.com/brandName\"==\"a\"..\"z\"");
        assert_eq!(sql.matches("(\"ca\".\"value_string\") COLLATE \"und-x-icu\"").count(), 2, "{sql}");
        assert_eq!(sql.matches("(\"ca\".\"value_long_text\") COLLATE \"und-x-icu\"").count(), 2, "{sql}");
    }

    /// Clause 4.9 follows every object of the Relationship: one object in
    /// either URI column, several as the JSON array in `value_object`, each
    /// expanded to one row and joined to the entity it names.
    #[test]
    fn a_hop_expands_every_object_of_the_relationship() {
        let (sql, _) = render("\"https://example.com/isParkedIn\"{\"https://example.com/name\"}==\"x\"");
        assert!(
            sql.contains(r#"JOIN jsonb_array_elements_text(COALESCE("ca_ln1"."value_object", jsonb_build_array(COALESCE("ca_ln1"."value_uri", "ca_ln1"."value_long_uri")))) AS "lo1" ON TRUE"#),
            "{sql}"
        );
        assert!(sql.contains(r#"JOIN "entities" AS "le1" ON "le1"."id" = "lo1""#), "{sql}");
        assert!(
            sql.contains(r#""ca_ln1"."attribute_kind" IN (2, 8)"#),
            "only a Relationship or a ListRelationship holds objects: {sql}"
        );
    }

    /// Clause 4.9: an operand is matched when it is included in the target value
    /// of a Relationship holding several objects, the JSON array of URIs in
    /// `value_object`.
    #[test]
    fn a_relationship_equality_reads_an_object_among_several() {
        let (sql, _) = render("\"https://example.com/isParkedIn\"==\"urn:ngsi-ld:Part:c\"");
        assert!(sql.contains(r#""ca"."value_uri" = "#), "{sql}");
        assert!(sql.contains(r#""ca"."value_object" @> jsonb_build_array("#), "{sql}");
    }

    /// A Relationship object past the B-tree bound is held in `value_long_uri`,
    /// and an operand of that length is compared there.
    #[test]
    fn a_long_relationship_object_is_compared_in_its_overflow_column() {
        let object = format!("urn:ngsi-ld:Part:{}", "x".repeat(1100));
        let (sql, _) = render(&format!("\"https://example.com/isParkedIn\"==\"{object}\""));
        assert!(sql.contains(r#""ca"."value_long_uri" = "#), "{sql}");
        assert!(!sql.contains(r#""ca"."value_uri" = "#), "{sql}");
    }

    /// The equality operator keeps the plain comparison: clause 4.9 states the
    /// datatype rule the other way for it: "If there is no equality between the
    /// target value data type and the Query Term value data type, then it shall
    /// be considered as not matching", which is what an unknown already means.
    #[test]
    fn an_equality_is_not_negated_at_all() {
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(!sql.contains("NOT COALESCE("), "{sql}");
    }

    /// Clause 4.9's `unequal` over a value list: "The target value is neither
    /// identical nor equivalent to any of the list values", so every member's
    /// comparison is negated and the negations are conjoined.
    #[test]
    fn a_negated_value_list_negates_every_member_totally() {
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 2, "one total negation per member: {sql}");
        assert!(sql.contains(") AND ("), "the members are conjoined: {sql}");
    }

    /// Clause 4.9 states the same datatype rule for a range operand's negated
    /// form, so a target the interval cannot even be evaluated against is
    /// unequal to it rather than dropped.
    #[test]
    fn a_negated_range_negates_the_interval_test_totally() {
        let (sql, _) = render(&format!("{}!=10..20", quoted(SPEED)));
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" >= "), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" <= "), "{sql}");
    }

    /// A language-selected inequality keeps its own compilation: clause 4.9
    /// requires the entity to contain the target element, the
    /// `LanguageProperty`, so a scalar-valued attribute of the same name is no
    /// match, while a `languageMap` without the key is one ("No matching value
    /// is found as the value of the specified language key").
    #[test]
    fn a_language_selected_inequality_requires_a_language_property_rather_than_the_key() {
        let (sql, _) = render("\"https://example.com/color\"[en]!=\"red\"");
        assert!(
            sql.contains("(EXISTS(SELECT 1 FROM jsonb_array_elements((CASE WHEN (\"ca\".\"attribute_kind\" = (5))"),
            "the LanguageProperty guard: {sql}"
        );
        assert!(!sql.contains("jsonb_build_object('@language', $2::text)) "), "no key guard: {sql}");
        assert!(
            !sql.contains("\"ca\".\"value_string\" = "),
            "no typed column answers a language-selected path: {sql}"
        );
    }

    #[test]
    fn no_probe_guards_against_a_stored_deletion_marker() {
        // A deleted attribute is an absent row, so there is
        // nothing to exclude and no sentinel value a client could collide with.
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(!sql.contains("IS DISTINCT FROM"), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:null"), "{sql}");
    }

    #[test]
    fn no_probe_filters_on_the_value_kind_code() {
        // The row's check constraint already pins the kind to the
        // populated column, and a `value_kind = k` term would turn the NULL a
        // wrong-kind row yields into FALSE, which a negated predicate then
        // reads as a match.
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert!(!sql.contains("value_kind"), "{sql}");
    }

    #[test]
    fn boolean_comparison_binds_native_boolean() {
        let (sql, binds) = render("\"https://example.com/active\"==true");
        assert!(sql.contains("\"ca\".\"value_boolean\" ="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, Value::Bool(Some(true)))), "{binds:?}");
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let (sql, binds) = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert!(sql.contains("\"ca\".\"value_date_time\" >="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, Value::ChronoDateTimeUtc(_))), "{binds:?}");
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_projection_stores_it_that_way() {
        let (sql, _) = render("\"https://example.com/d\"==2024-01-01");
        assert!(sql.contains("\"ca\".\"value_string\" ="), "{sql}");
        assert!(!sql.contains("value_date_time"), "only full date-times reach value_date_time: {sql}");
    }

    #[test]
    fn a_time_operand_binds_the_canonical_form_the_write_path_stores() {
        // Clause 4.6.3 admits a fraction of one to six digits, written with a
        // decimal point or a comma, and the stored text is normalised to one
        // canonical rendering of all of them. The probe has to bind that same
        // rendering or a short-fraction operand matches nothing.
        let (_, binds) = render("\"https://example.com/d\"==\"12:00:00,5Z\"");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "12:00:00.500000Z")),
            "{binds:?}"
        );
    }

    #[test]
    fn a_time_ordering_comparison_binds_the_canonical_form_too() {
        // Byte-wise order is chronological exactly because both sides are the
        // fixed-width form, the stored side normalised from the text as written.
        let (sql, binds) = render("\"https://example.com/d\">\"09:30:00.5Z\"");
        assert!(sql.contains(&format!("'{STORED_TIME_TEMPLATE}') || ('Z') END) > $2")), "{sql}");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "09:30:00.500000Z")),
            "{binds:?}"
        );
    }

    #[test]
    fn inclusive_range_becomes_a_bounded_pair_inside_one_probe() {
        let (sql, binds) = render("\"https://example.com/temperature\"==10..20");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not two: {sql}");
        assert!(sql.contains("\"ca\".\"value_number\" >="), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" <="), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(12))));
        assert!(matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "10"));
        assert!(matches!(&binds[2], Value::BigDecimal(Some(value)) if value.to_string() == "20"));
    }

    #[test]
    fn excluded_range_requires_the_attribute_to_exist() {
        let (sql, _) = render("\"https://example.com/temperature\"!=10..20");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" <"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_number\" >"), "{sql}");
    }

    #[test]
    fn value_list_becomes_a_disjunction_inside_one_probe() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn a_negated_value_list_is_a_conjunction_of_negations_not_a_negated_existence() {
        // Clause 4.9: the entity must still contain the target element, and the
        // target value must differ from every member of the list. Negating the
        // `EXISTS` instead would match an entity that simply lacks the
        // attribute; each member is negated inside the one probe.
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert!(sql.contains("EXISTS"), "the attribute must still exist: {sql}");
        assert!(!sql.contains("NOT (EXISTS"), "{sql}");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 2, "one negation per member: {sql}");
    }

    #[test]
    fn integers_beyond_binary64_precision_use_the_numeric_projection() {
        let (sql, binds) = render(&format!("{}==9007199254740993", quoted(SPEED)));
        assert!(sql.contains("current_attributes"), "{sql}");
        assert!(!sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "9007199254740993"),
            "{binds:?}"
        );
    }

    #[test]
    fn exact_fractions_use_the_projection() {
        let (sql, _) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert!(sql.contains("\"ca\".\"value_number\" >"), "{sql}");
    }

    #[test]
    fn pattern_match_uses_posix_regex_on_the_string_column() {
        let (sql, _) = render("\"https://example.com/name\"~=\"^A.*\"");
        assert!(sql.contains("\"ca\".\"value_string\" ~"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_long_text\" ~"), "{sql}");
    }

    #[test]
    fn negated_pattern_sits_inside_the_probe_so_missing_attributes_do_not_match() {
        let (sql, _) = render("\"https://example.com/name\"!~=\"^A.*\"");
        assert!(sql.contains("\"ca\".\"value_string\" !~"), "{sql}");
        assert!(sql.contains("\"ca\".\"value_long_text\" !~"), "{sql}");
        assert!(
            !sql.contains("NOT (EXISTS(SELECT 1 FROM \"current_attributes\"") && !sql.contains("NOT EXISTS(SELECT 1 FROM \"current_attributes\""),
            "clause 4.9 requires the target element to be present: {sql}"
        );
    }

    /// Clause 4.9: with no target language specified, a `LanguageProperty`'s
    /// target value is "a value from any of the key-value pairs held within the
    /// languageMap", and a `ListProperty`'s is "the valueList array", both of
    /// which the write path stores in `value_object`, so an equality Query
    /// Term has to reach that column and not only the typed ones.
    #[test]
    fn an_unqualified_equality_reaches_the_structured_value_column() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\"");
        assert!(sql.contains("\"ca\".\"value_string\" = "), "{sql}");
        assert!(sql.contains("\"ca\".\"value_object\" @> jsonb_build_array($"), "{sql}");
        assert!(
            sql.contains("\"ca\".\"value_object\" @> jsonb_build_array(jsonb_build_object('@value', $"),
            "the value-node element shape a languageMap and a valueList carry: {sql}"
        );
    }

    /// Clause 4.9: "the target object of a `ListRelationship` target element is
    /// the array of objects (URIs) associated with it", and a URI Query Term
    /// matches when it "is included in the target value". The objects are
    /// projected as bare array elements, which is what the unwrapped
    /// containment shape reaches.
    #[test]
    fn a_uri_query_term_reaches_an_object_list_through_the_bare_element_shape() {
        let (sql, binds) = render("\"https://example.com/color\"==\"urn:ngsi-ld:Part:1\"");
        assert!(sql.contains("\"ca\".\"value_object\" @> jsonb_build_array($"), "{sql}");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "urn:ngsi-ld:Part:1")),
            "{binds:?}"
        );
    }

    /// Clause 4.9: "the target value is defined as the Value associated to the
    /// matching key-value pair held within the languageMap ... where the key
    /// matches the target language". The row decides whether `attr[en]` means
    /// that or a compound value member, so both are compiled, as one test of
    /// the attribute's rows.
    #[test]
    fn a_target_language_compiles_a_language_map_reading_beside_the_member_reading() {
        let (sql, _) = render("\"https://example.com/color\"[en]==\"red\"");
        assert!(
            sql.contains("jsonb_build_object('@language', $2::text, '@value', $3::text)"),
            "the languageMap reading: {sql}"
        );
        assert!(sql.contains(r#"jsonb_path_query("ca"."value_object""#), "the compound-value reading: {sql}");
        assert!(sql.contains(" OR "), "{sql}");
        assert_eq!(sql.matches("FROM \"current_attributes\"").count(), 1, "{sql}");
    }

    /// A language-selected path addresses a `languageMap` key. A scalar-valued
    /// attribute of the same name has no such key, so clause 4.9's target
    /// element does not exist for it and neither polarity may consult its typed
    /// column: `color[en]!=5` must not select a plain numeric Property.
    #[test]
    fn a_target_language_never_consults_the_typed_scalar_columns() {
        for expression in ["\"https://example.com/color\"[en]==5", "\"https://example.com/color\"[en]!=5"] {
            let (sql, _) = render(expression);
            assert!(!sql.contains("\"ca\".\"value_number\""), "{expression}: {sql}");
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
        let (sql, _) = render("\"https://example.com/color\"!=\"red\"");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 1, "{sql}");
        assert!(sql.contains("\"ca\".\"value_object\" @> "), "the structured readings are inside it: {sql}");
        assert!(sql.contains("\"ca\".\"value_string\" = "), "so are the typed columns: {sql}");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(!sql.contains("NOT (EXISTS"), "{sql}");
    }

    /// Clause 4.9 admits a value list against every target: "The target value
    /// includes any of the Query Term values, if the target value is an array".
    #[test]
    fn a_value_list_reaches_the_structured_value_column_once_per_member() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_eq!(sql.matches("\"ca\".\"value_object\" @> ").count(), 4, "two shapes per member: {sql}");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not one per member: {sql}");
    }

    /// The `unequal` complement of the value list: none of the members may be
    /// included in the structured target value.
    #[test]
    fn a_negated_value_list_denies_the_structured_value_column_for_every_member() {
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert_eq!(sql.matches("\"ca\".\"value_object\" @> ").count(), 4, "two element shapes per member: {sql}");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 2, "one negation per member: {sql}");
    }

    /// Clause 4.9 requires a matching Entity to contain the target element,
    /// which for a language-selected path is the key-value pair itself.
    #[test]
    fn a_bare_existence_of_a_target_language_probes_the_language_key() {
        let (sql, _) = render("\"https://example.com/color\"[en]");
        assert!(sql.contains("jsonb_build_object('@language', $2::text)"), "{sql}");
        assert!(!sql.contains("'@value'"), "existence says nothing about the value: {sql}");
        assert!(
            sql.contains(r#"jsonb_path_query("ca"."value_object""#),
            "the compound-member reading is compiled beside it: {sql}"
        );
    }

    /// Clause 4.9's ordering, pattern and range operators are not containment
    /// tests, so nothing probes the structured column by containment under
    /// them. A `LanguageProperty`'s strings are each a String target value with
    /// no target language, so a String operand reads them one by one, only on
    /// a row written as a `LanguageProperty`, never a `ListProperty`'s array.
    #[test]
    fn ordering_pattern_and_string_range_read_a_language_map_only_on_a_language_property() {
        for expression in [
            "\"https://example.com/color\">\"red\"",
            "\"https://example.com/color\"~=\"re.*\"",
            "\"https://example.com/color\"!~=\"re.*\"",
            "\"https://example.com/color\"==\"a\"..\"m\"",
            "\"https://example.com/color\"!=\"a\"..\"m\"",
        ] {
            let (sql, _) = render(expression);
            assert!(!sql.contains("\"value_object\" @>"), "{expression}: {sql}");
            assert!(
                sql.contains(r#"jsonb_array_elements((CASE WHEN ("ca"."attribute_kind" = (5)) THEN "ca"."value_object" END))"#),
                "{expression}: {sql}"
            );
            assert!(sql.contains("\"ca\".\"value_string\""), "the typed column is still read: {expression}: {sql}");
        }
    }

    /// A number or a boolean is a datatype no string carries, so its ordering
    /// and range terms read the typed column alone and keep its index.
    #[test]
    fn a_numeric_ordering_or_range_reads_only_its_typed_column() {
        for expression in [
            "\"https://example.com/temperature\">20",
            "\"https://example.com/temperature\"==10..20",
            "\"https://example.com/temperature\"!=10..20",
        ] {
            let (sql, _) = render(expression);
            assert!(!sql.contains("value_object"), "{expression}: {sql}");
        }
    }

    /// Clause 4.9: an excluded range holds over a language map none of whose
    /// strings lies in the interval, which is the total negation of the one
    /// interval test that reads the typed column and the strings together.
    #[test]
    fn an_excluded_string_range_negates_the_strings_with_the_typed_column() {
        let (sql, _) = render("\"https://example.com/color\"!=\"a\"..\"m\"");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 1, "{sql}");
        let negated = sql.split("NOT COALESCE(").nth(1).unwrap_or_default();
        assert!(negated.contains("jsonb_array_elements"), "the strings sit inside the negation: {sql}");
    }

    /// Clause 4.9: with a target language the target value is a String of that
    /// language, so the ordering, pattern and range operators read its strings
    /// on the projection, disjoined with the compound-member reading.
    #[test]
    fn a_target_language_reads_that_languages_strings_under_every_operator() {
        for expression in [
            "\"https://example.com/color\"[en]>\"m\"",
            "\"https://example.com/color\"[en]~=\"^r\"",
            "\"https://example.com/color\"[en]!~=\"^r\"",
            "\"https://example.com/color\"[en]==\"a\"..\"m\"",
            "\"https://example.com/color\"[en]!=\"a\"..\"m\"",
        ] {
            let (sql, binds) = render(expression);
            assert!(sql.contains(r#""ls"."value" ->> ('@language')"#), "{expression}: {sql}");
            assert!(
                binds.iter().any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "en")),
                "the tag binds as the client wrote it: {expression}: {binds:?}"
            );
            assert!(sql.contains("jsonb_path_"), "the compound-member reading stays: {expression}: {sql}");
            assert!(sql.contains(" OR "), "{expression}: {sql}");
        }
    }

    /// The complementary forms require the `LanguageProperty`, not the target
    /// language, and deny every string of that language (clause 4.9: "No
    /// matching value is found as the value of the specified language key").
    #[test]
    fn a_target_language_complement_requires_the_language_property_rather_than_the_language() {
        for expression in ["\"https://example.com/color\"[en]!~=\"^r\"", "\"https://example.com/color\"[en]!=\"a\"..\"m\""] {
            let (sql, _) = render(expression);
            assert!(sql.contains("NOT EXISTS(SELECT 1 FROM jsonb_array_elements"), "{expression}: {sql}");
            assert_eq!(
                sql.matches(r#""ls"."value" ->> ('@language')"#).count(),
                1,
                "only the denial pins the tag: {expression}: {sql}"
            );
        }
    }

    /// Clause 4.9's target element is the last name of the dotted path, so a
    /// `LanguageProperty` sub-attribute is read through the `languageMap` its
    /// element holds, beside the reading of a compound member.
    #[test]
    fn a_sub_attribute_is_read_through_the_language_map_of_its_element() {
        for expression in [
            "\"https://example.com/address\".\"https://example.com/label\"[en]==\"Front\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]!=\"Front\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]",
            "\"https://example.com/address\".\"https://example.com/label\"[en]>\"F\"",
            "\"https://example.com/address\".\"https://example.com/label\"[en]~=\"^F\"",
            "\"https://example.com/address\".\"https://example.com/label\"==\"Front\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains(r#"UNNEST("ca"."sub_attributes") AS "l0""#), "{expression}: {sql}");
            assert!(sql.contains(r#""l0"."value_object""#), "{expression}: {sql}");
            assert!(!sql.contains("hasLanguageMap"), "{expression}: {sql}");
        }
    }

    /// A nested compound path below a sub-attribute has no language reading.
    #[test]
    fn a_nested_compound_path_below_a_sub_attribute_reads_no_language_map() {
        let (sql, _) = render("\"https://example.com/address\".\"https://example.com/label\"[a.b]==\"x\"");
        assert!(!sql.contains("'@language'"), "{sql}");
        assert!(sql.contains(r#"jsonb_path_query("l0"."value_object""#), "{sql}");
    }

    #[test]
    fn unknown_attribute_iri_compiles_to_a_constant_false() {
        let (sql, _) = render_with(
            &format!("\"{}\">1", "https://example.com/never-written"),
            &names(),
            &EntityRowRefs::unaliased(None),
        );
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("current_attributes"), "no probe is emitted when no row can exist: {sql}");
    }

    #[test]
    fn bare_existence_is_the_presence_of_a_projection_row() {
        // A deleted attribute leaves no row, so presence needs no second probe
        // and no tombstone test.
        let (sql, binds) = render(&quoted(SPEED));
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attributes" AS "ca" "#,
                r#"WHERE "ca"."attribute_name_key" = $1 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) "#,
                r#"AND "ca"."entity_key" = "entities"."entity_key")"#,
            )
        );
        assert!(matches!(binds[0], Value::Int(Some(7))), "{binds:?}");
    }

    #[test]
    fn unknown_attribute_existence_compiles_to_a_constant_false() {
        let (sql, _) = render("\"https://example.com/never-written\"");
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("current_attributes"), "{sql}");
    }

    #[test]
    fn and_or_nesting_preserves_grouping() {
        let (sql, _) = render(&format!(
            "({}>1;\"https://example.com/b\"<2)|\"https://example.com/c\"==\"x\"",
            quoted("https://example.com/a")
        ));
        assert_eq!(sql.matches("EXISTS").count(), 3, "one probe per predicate: {sql}");
        assert!(sql.contains(" AND "), "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    /// Some instance matching one Query Term or some instance matching another
    /// is some instance matching either (clause 4.9), so a disjunction is one
    /// probe whose arms each keep their own attribute key.
    #[test]
    fn a_disjunction_of_terms_shares_one_probe() {
        for expression in [
            "\"https://example.com/color\"==\"red\"|\"https://example.com/color\"==\"blue\"",
            "\"https://example.com/color\"==\"red\"|\"https://example.com/name\"!=\"x\"|\"https://example.com/a\">1",
            "\"https://example.com/color\"|\"https://example.com/name\"~=\"^A\"",
            "(\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"x\")|\"https://example.com/a\"==1",
        ] {
            let (sql, _) = render(expression);
            assert_eq!(sql.matches("FROM \"current_attributes\"").count(), 1, "{expression}: {sql}");
            assert!(
                sql.contains("EXISTS(SELECT 1 FROM \"current_attributes\" AS \"ca\" WHERE (\"ca\".\"expires_at\" IS NULL"),
                "the attribute key moves into each arm: {expression}: {sql}"
            );
        }
    }

    /// The arms of a shared probe bind in the order the terms were written, each
    /// attribute key ahead of its own operands.
    #[test]
    fn a_shared_probe_binds_each_attribute_key_ahead_of_its_operands() {
        let (sql, binds) = render("\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"x\"");
        assert!(sql.contains("(\"ca\".\"attribute_name_key\" = $1 AND "), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(13))), "{binds:?}");
        let second = binds
            .iter()
            .position(|bind| matches!(bind, Value::Int(Some(14))))
            .expect("the second key binds");
        assert!(
            binds[1..second]
                .iter()
                .all(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "red")),
            "{binds:?}"
        );
    }

    /// A linked-entity hop reads other rows than the scan does, so it stands
    /// beside the shared probe, which a compound member reading joins: it reads
    /// the `value_object` of a row of its attribute.
    #[test]
    fn hops_stand_beside_the_shared_probe_and_member_readings_join_it() {
        let (sql, _) = render(&format!(
            "{}{{{}}}==\"x\"|\"https://example.com/color\"==\"red\"|\"https://example.com/name\"==\"y\"|\"https://example.com/address\"[street]>\"a\"",
            quoted(IS_PARKED_IN),
            quoted(CATEGORY)
        ));
        assert!(sql.contains("\"ca_ln1\""), "the hop keeps its own probe: {sql}");
        assert!(sql.contains(r#"jsonb_path_query("ca"."value_object""#), "the member reading: {sql}");
        assert_eq!(
            sql.matches("FROM \"current_attributes\" AS \"ca\" WHERE (\"ca\".\"expires_at\"").count(),
            1,
            "the three row terms share one probe: {sql}"
        );
    }

    /// A range on a sub-attribute has no key to read, so its probe walks: it
    /// carries an `OFFSET`, which keeps it a per-row probe in page order, and
    /// rechecks the element inline.
    #[test]
    fn a_sub_attribute_range_walks() {
        let (sql, _) = render("\"https://example.com/address\".\"https://example.com/label\">0.5");
        assert!(!sql.contains("&&"), "{sql}");
        assert!(sql.contains(r#"UNNEST("ca"."sub_attributes") AS "l0""#), "{sql}");
        assert!(sql.contains(r#""l0"."value_number" > "#), "{sql}");
        assert!(sql.contains("OFFSET"), "the probe is walked: {sql}");
    }

    /// No trigram index covers an element's strings, so a pattern on a
    /// sub-attribute walks and reads the element's one string field.
    #[test]
    fn a_sub_attribute_pattern_walks_without_a_trigram_filter() {
        let (sql, _) = render("\"https://example.com/address\".\"https://example.com/label\"~=\"^x\"");
        assert!(!sql.contains("&&") && !sql.contains(" % ") && !sql.contains("similarity"), "{sql}");
        assert!(sql.contains(r#""l0"."value_string" ~ "#), "{sql}");
        assert!(!sql.contains("value_long_text"), "{sql}");
        assert!(sql.contains("OFFSET"), "{sql}");
    }

    /// A top-level pattern is matched whole: PostgreSQL extracts the trigrams
    /// `idx_ca_value_string_trgm` filters on from the pattern itself, anchors
    /// included, and newline-sensitive mode is never set.
    #[test]
    fn a_top_level_pattern_keeps_its_anchors_and_never_sets_newline_mode() {
        let (sql, binds) = render("\"https://example.com/color\"~=\"^x-[0-9]$\"");
        assert!(sql.contains(r#""ca"."value_string" ~ "#), "{sql}");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "^x-[0-9]$")),
            "{binds:?}"
        );
        assert!(
            !binds.iter().any(|bind| matches!(bind, Value::String(Some(text)) if text.contains("(?n)"))),
            "{binds:?}"
        );
        assert!(!sql.contains("OFFSET"), "a row term stays a planned probe: {sql}");
    }

    /// A member of a sub-attribute's compound value is read out of the
    /// element's `value_object`, which no key covers, so the term walks.
    #[test]
    fn a_value_object_sub_attribute_walks() {
        let (sql, _) = render("\"https://example.com/address\".\"https://example.com/label\"[c]==\"x\"");
        assert!(!sql.contains("&&"), "{sql}");
        assert!(sql.contains(r#"jsonb_path_query("l0"."value_object""#), "{sql}");
        assert!(sql.contains("OFFSET"), "{sql}");
    }

    /// A path ending on a member of the attribute reads the field of its row,
    /// with no element chain at all.
    #[test]
    fn a_member_of_the_attribute_is_read_from_its_row() {
        let (sql, _) = render(&format!(
            "{}.\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-01-01T00:00:00Z\"",
            quoted(SPEED)
        ));
        assert!(sql.contains(r#""ca"."observed_at" > "#), "{sql}");
        assert!(!sql.contains("UNNEST"), "{sql}");
    }

    /// Two terms one instance must pass together are not two terms two
    /// instances pass separately, so a conjunction keeps a probe per term.
    #[test]
    fn a_conjunction_keeps_one_probe_per_term() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\";\"https://example.com/name\"==\"x\"");
        assert_eq!(sql.matches("EXISTS").count(), 2, "{sql}");
    }

    #[test]
    fn a_correlated_statement_probes_against_the_entity_key_of_its_own_alias() {
        let (sql, _) = render_with(
            &format!("{}>50", quoted(SPEED)),
            &names(),
            &EntityRowRefs::aliased(None, Alias::new("ce"), Alias::new("cs")),
        );
        assert!(sql.contains("\"ca\".\"entity_key\" = \"ce\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_surrogate_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"ca\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("\"entities\".\"id\""), "the URI is not the correlation column: {sql}");
    }

    const IS_PARKED_IN: &str = "https://example.com/isParkedIn";
    const CATEGORY: &str = "https://example.com/category";

    #[test]
    fn a_single_hop_probes_the_relationship_and_joins_the_entity_it_names() {
        let (sql, binds) = render(&format!("{}{{{}}}==\"commercial\"", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attributes\" AS \"ca_ln1\""), "{sql}");
        assert!(sql.contains("JOIN \"entities\" AS \"le1\" ON \"le1\".\"id\" = \"lo1\""), "{sql}");
        assert!(sql.contains("\"ca_ln1\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("entity_search"), "a hop without a type guard reads no search row: {sql}");
        // The inner comparison is correlated against the linked entity's key,
        // which is the row the join on its URI landed on.
        assert!(sql.contains("\"ca\".\"entity_key\" = \"le1\".\"entity_key\""), "{sql}");
        assert!(sql.contains("\"ca\".\"value_string\" ="), "{sql}");
        assert!(
            matches!(binds[0], Value::Int(Some(19))),
            "the relationship's dictionary key binds first: {binds:?}"
        );
        assert!(matches!(binds[1], Value::Int(Some(20))), "then the inner attribute's: {binds:?}");
    }

    #[test]
    fn a_hop_guard_tests_the_linked_entitys_types_for_any_of_its_alternatives() {
        // Clause 4.9 lists the guard's types as alternatives, so one membership
        // test against `entities.entity_type_name_keys` per resolved type, disjoined.
        let (sql, binds) = render(&format!(
            "{}{{{},{}:{}}}==\"x\"",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Store"),
            quoted(CATEGORY)
        ));
        // The whole statement is pinned, not a substring: the guard renders as a
        // bare `a OR b`, and without parentheses around it the second disjunct
        // escapes the correlation, the attribute-name test and the tombstone
        // guard, and the probe would then match whenever any entity in the tenant
        // carried the second type. A `contains` assertion cannot see that.
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attributes" AS "ca_ln1" "#,
                r#"JOIN jsonb_array_elements_text(COALESCE("ca_ln1"."value_object", jsonb_build_array(COALESCE("ca_ln1"."value_uri", "ca_ln1"."value_long_uri")))) AS "lo1" ON TRUE "#,
                r#"JOIN "entities" AS "le1" ON "le1"."id" = "lo1" "#,
                r#"JOIN "entity_search" AS "ls1" ON "ls1"."entity_key" = "le1"."entity_key" "#,
                r#"WHERE "ca_ln1"."entity_key" = "entities"."entity_key" "#,
                r#"AND "ca_ln1"."attribute_name_key" = $1 "#,
                r#"AND "ca_ln1"."attribute_kind" IN (2, 8) "#,
                r#"AND ("ca_ln1"."expires_at" IS NULL OR "ca_ln1"."expires_at" > NOW()) "#,
                r#"AND (("ls1"."entity_type_name_keys" @> (ARRAY[31])) OR ("ls1"."entity_type_name_keys" @> (ARRAY[32]))) "#,
                r#"AND EXISTS(SELECT 1 FROM "current_attributes" AS "ca" "#,
                r#"WHERE "ca"."attribute_name_key" = $2 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) "#,
                r#"AND ("ca"."value_string" = $3 OR "ca"."value_uri" = $4 "#,
                r#"OR (("ca"."value_object" @> jsonb_build_array($5::text)) "#,
                r#"OR ("ca"."value_object" @> jsonb_build_array(jsonb_build_object('@value', $6::text))))) "#,
                r#"AND "ca"."entity_key" = "le1"."entity_key"))"#,
            )
        );
        assert!(
            !binds.iter().any(|bind| matches!(bind, Value::Int(Some(31 | 32)))),
            "the guard type keys are inlined for the planner, never bound: {binds:?}"
        );
        assert!(
            !binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.contains("example.com/Building"))),
            "the type IRI itself never reaches the statement: {binds:?}"
        );
    }

    /// A guard type the dictionary never recorded was never carried by any
    /// entity, so that alternative is unsatisfiable while the resolved one
    /// still selects.
    #[test]
    fn an_unresolved_guard_type_renders_as_an_unsatisfiable_alternative() {
        let (sql, _) = render(&format!(
            "{}{{{},{}:{}}}==\"x\"",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Warehouse"),
            quoted(CATEGORY)
        ));
        assert!(sql.contains(r#"(("ls1"."entity_type_name_keys" @> (ARRAY[31])) OR (FALSE))"#), "{sql}");
    }

    #[test]
    fn an_unguarded_hop_emits_no_type_test() {
        let (sql, _) = render(&format!("{}{{{}}}==\"commercial\"", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(!sql.contains("\"entity_type_name_keys\""), "no guard was written: {sql}");
    }

    #[test]
    fn nested_hops_number_their_aliases_by_depth() {
        let (sql, _) = render(&format!(
            "{}{{{}{{{}}}}}==\"north\"",
            quoted("https://example.com/outer"),
            quoted("https://example.com/inner"),
            quoted("https://example.com/zone")
        ));
        for alias in ["ca_ln1", "lo1", "le1", "ca_ln2", "lo2", "le2"] {
            assert!(sql.contains(&format!("AS \"{alias}\"")), "{alias} missing from {sql}");
        }
        assert!(
            sql.contains("\"ca_ln2\".\"entity_key\" = \"le1\".\"entity_key\""),
            "the second hop starts at the first's target: {sql}"
        );
        assert!(sql.contains("\"ca\".\"entity_key\" = \"le2\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn a_bare_existence_through_a_hop_probes_the_rows_of_the_linked_entity() {
        let (sql, _) = render(&format!("{}{{{}}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attributes\" AS \"ca_ln1\""), "{sql}");
        assert!(
            sql.contains("\"ca\".\"entity_key\" = \"le1\".\"entity_key\""),
            "existence resolves against the entity the hop landed on: {sql}"
        );
    }

    #[test]
    fn an_unresolved_relationship_name_makes_the_traversal_unsatisfiable() {
        // No `attribute_names` row means no instance of the Relationship was
        // ever written, so no entity is reachable through it.
        let (sql, _) = render(&format!("{}{{{}}}==\"x\"", quoted("https://example.com/never-written"), quoted(CATEGORY)));
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("ca_ln1"), "no probe is emitted when no row can exist: {sql}");
    }
}
