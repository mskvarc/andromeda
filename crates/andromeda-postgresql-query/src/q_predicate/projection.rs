//! `q` predicates compiled to `EXISTS` probes against the
//! `current_attribute_values` projection.

use crate::{
    attribute_name_id::AttributeNameId,
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    operator::{does_not_hold, pattern as pattern_predicate},
    q_predicate::{
        fallback::{fallback_equality, fallback_exists, fallback_ordering, fallback_pattern},
        local_target::{LocalTarget, local_target},
        projection_probe::{attribute_row_exists, exists_on_projection, probe_column},
        structured_target::{StructuredTarget, carries_operand, carries_target_language, omits_operand},
        traversal::LinkedHop,
        typed_column::typed_column_condition,
    },
};
use andromeda_ngsi_ld_query::{
    filter::{
        expression::Expr,
        path::{AttrPath, QueryPath},
        predicate::{CmpOp, OrderingOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// Compile a parsed `q` expression into a sea-query [`Condition`].
///
/// Compilation cannot fail: an attribute or entity type name the dictionary
/// does not hold compiles to a constant `FALSE` rather than erroring, and every
/// operand is bound as a parameter rather than converted, so every expression
/// clause 4.9 admits has a condition.
#[must_use]
pub fn compile(expr: &Expr, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match expr {
        Expr::And(terms) => terms.iter().fold(Condition::all(), |cond, term| cond.add(compile(term, rows, names))),
        Expr::Or(terms) => terms.iter().fold(Condition::any(), |cond, term| cond.add(compile(term, rows, names))),
        Expr::Pred(pred) => compile_predicate(pred, rows, names),
    }
}

/// Compile one leaf predicate, descending any linked-entity hops its axis
/// carries before compiling the comparison against the entity it lands on.
fn compile_predicate(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    compile_along_path(pred, pred.path(), rows, names, 0)
}

/// Compile a predicate against the entity reached by the remaining axis.
///
/// `depth` is the number of hops already descended; it suffixes the aliases so
/// a nested traversal's probes cannot shadow an enclosing one's.
fn compile_along_path(pred: &Predicate, path: &QueryPath, rows: &EntityRowRefs, names: &QueryNames, depth: usize) -> Condition {
    match path {
        QueryPath::Local(local) => compile_local_predicate(pred, local, rows, names),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let Some(relationship_id) = names.attributes.get(relationship) else {
                // No `attribute_names` row means no instance of the
                // Relationship was ever written, so no entity can be reached
                // through it.
                return unsatisfiable();
            };
            let hop = LinkedHop::at_depth(depth + 1);
            let inner_condition = compile_along_path(pred, inner, &hop.inner_rows(), names, depth + 1);
            hop.probe(relationship_id, guard_types, &names.types, rows, inner_condition)
        }
    }
}

/// Compile the comparison itself, routing it to the projection or the fallback.
fn compile_local_predicate(pred: &Predicate, path: &AttrPath, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match pred {
        Predicate::Exists(_) => compile_exists(path, rows, names),
        Predicate::Equality { membership, rhs, .. } => compile_equality(path, *membership, rhs, rows, names),
        Predicate::Ordering { op, rhs, .. } => match local_target(path) {
            LocalTarget::OwnValue => match names.attributes.get(&path.attr) {
                Some(id) => compile_ordering_on_projection(id, *op, rhs, rows),
                None => unsatisfiable(),
            },
            // An ordering comparison against one member of a structure is not a
            // containment test, so nothing in the projection answers it and the
            // payload is the only reader left.
            LocalTarget::TrailingMember(_) | LocalTarget::PayloadMember => fallback_ordering(path, *op, rhs, rows),
        },
        Predicate::MatchPattern { pattern, .. } => compile_pattern(path, pattern, PatternPolarity::Match, rows, names),
        Predicate::NotMatchPattern { pattern, .. } => compile_pattern(path, pattern, PatternPolarity::NotMatch, rows, names),
    }
}

/// Compile an equality Query Term (`==`/`!=`), routing it by what its path
/// addresses.
///
/// A trailing member is compiled twice and disjoined, because clause 4.9 admits
/// two readings of `attr[member]` and only the stored row says which one is
/// meant: the projection probe answers the `LanguageProperty` reading, where the
/// member is the target language, and the payload fallback answers the compound
/// value reading. Neither can match the other's rows — a `languageMap` carries
/// no `hasValue` for the fallback's jsonpath to reach, and a compound value's
/// member nodes carry no `@language` for the containment to match — so the
/// disjunction is exact rather than merely permissive.
fn compile_equality(path: &AttrPath, membership: SetMembership, rhs: &CompareValue, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match local_target(path) {
        LocalTarget::OwnValue => match names.attributes.get(&path.attr) {
            Some(id) => compile_equality_on_projection(id, StructuredTarget::AnyValue, membership, rhs, rows),
            None => unsatisfiable(),
        },
        LocalTarget::TrailingMember(member) => {
            let tagged = match names.attributes.get(&path.attr) {
                Some(id) => compile_equality_on_projection(id, StructuredTarget::TargetLanguage(member), membership, rhs, rows),
                None => unsatisfiable(),
            };
            Condition::any().add(tagged).add(fallback_equality(path, membership, rhs, rows))
        }
        LocalTarget::PayloadMember => fallback_equality(path, membership, rhs, rows),
    }
}

/// A predicate that no row can satisfy.
fn unsatisfiable() -> Condition {
    Condition::all().add(false_literal())
}

// ---------------------------------------------------------------------------
// Existence
// ---------------------------------------------------------------------------

/// Compile a bare existence predicate.
///
/// A row in the projection *is* the attribute: the tombstone value kind belongs
/// to history and never appears in current state, so an attribute deleted by
/// clause 5.6.16 leaves no row behind and presence needs no further guard.
///
/// The obvious alternative, `payload->0 ? '<iri>'`, cannot use the GIN index on
/// `payload`: GIN's `jsonb_ops` only serves `?` when the left operand is the
/// bare indexed column, and `payload->0` is an expression. It is a sequential
/// scan.
fn compile_exists(path: &AttrPath, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match local_target(path) {
        LocalTarget::OwnValue => match names.attributes.get(&path.attr) {
            Some(id) => attribute_row_exists(id, rows),
            None => unsatisfiable(),
        },
        // Clause 4.9's target element for a language-selected path is the
        // matching key-value pair, so its presence is what existence asks —
        // and, on any other family, the presence of the compound member the
        // same spelling addresses.
        LocalTarget::TrailingMember(member) => {
            let tagged = match names.attributes.get(&path.attr) {
                Some(id) => exists_on_projection(id, carries_target_language(&probe_column(CurrentAttributeValues::ValueObject), member), rows),
                None => unsatisfiable(),
            };
            Condition::any().add(tagged).add(fallback_exists(path, rows))
        }
        LocalTarget::PayloadMember => fallback_exists(path, rows),
    }
}

// ---------------------------------------------------------------------------
// Comparisons against the projection
// ---------------------------------------------------------------------------

/// Compile an equality comparison (`==`/`!=`) against the columns of
/// `current_attribute_values` that can hold the clause 4.9 target value.
fn compile_equality_on_projection(
    id: AttributeNameId,
    target: StructuredTarget<'_>,
    membership: SetMembership,
    rhs: &CompareValue,
    rows: &EntityRowRefs,
) -> Condition {
    let value = match rhs {
        CompareValue::Scalar(scalar) => equality_condition(target, scalar, membership),
        CompareValue::Range { min, max } => range_condition(target, min, max, membership),
        // Clause 4.9: every member of a value list is compared with the equality
        // operator, so `==` holds when any of them matches and `!=` when none
        // does. The negated form is a conjunction of per-member inequalities
        // rather than `NOT (any member matches)`: only one value column of a row
        // is ever populated, so the other columns' comparisons are NULL, and
        // negating the whole disjunction turns that NULL into a non-match for
        // every entity — which made `attr!="a","b"` select nothing at all.
        CompareValue::ValueList(values) => match membership {
            SetMembership::Within => values.iter().fold(Condition::any(), |cond, value| {
                cond.add(equality_condition(target, value, SetMembership::Within))
            }),
            SetMembership::Outside => values.iter().fold(Condition::all(), |cond, value| {
                cond.add(equality_condition(target, value, SetMembership::Outside))
            }),
        },
    };
    exists_on_projection(id, value, rows)
}

/// One equality comparison against every column the target value can occupy.
///
/// A scalar target lives in one typed column; a `LanguageProperty`'s
/// `languageMap`, a `ListProperty`'s `valueList`, a `ListRelationship`'s
/// `objectList`, a multi-term `VocabProperty`'s `vocab` and an array-valued
/// Property all live in `value_object`. Clause 4.9 names each of them a target
/// value, so both readings are tested and disjoined; the projection's own check
/// constraint leaves exactly one value column populated per row, so at most one
/// disjunct is anything but NULL.
///
/// `unequal` is compiled as the total negation of that same disjunction rather
/// than as a disjunction of negated comparisons, which is what clause 4.9
/// requires of it: "If the data type of the target value and the data type of
/// the Query Term value are different, then they shall be considered unequal."
/// A row whose value sits in another typed column makes every comparison
/// unknown, and only a total negation reads that as the datatype difference it
/// is — `temp!="20"` over a numeric Property matched nothing while the clause
/// requires it to match. Its inner disjunction is the whole `equal` condition,
/// so the two operators cannot drift apart.
///
/// With a target language the typed columns are left out entirely. A
/// language-selected path addresses a `languageMap` key, and a scalar-valued
/// attribute of the same name carries no such key: comparing its typed column
/// would let `attr[en]!=5` match a plain numeric Property, whose target element
/// for that path does not exist at all. That polarity keeps its own compilation
/// for the same reason — it has to require the key to be present.
fn equality_condition(target: StructuredTarget<'_>, scalar: &Scalar, membership: SetMembership) -> Condition {
    let column = probe_column(CurrentAttributeValues::ValueObject);
    match target {
        StructuredTarget::AnyValue => match membership {
            SetMembership::Within => carries_any_target_value(&column, scalar),
            SetMembership::Outside => Condition::all().add(does_not_hold(carries_any_target_value(&column, scalar))),
        },
        StructuredTarget::TargetLanguage(language) => match membership {
            SetMembership::Within => carries_operand(&column, target, scalar),
            SetMembership::Outside => omits_operand(&column, language, scalar),
        },
    }
}

/// Whether any column that can hold an unqualified clause 4.9 target value
/// holds this operand.
fn carries_any_target_value(column: &SimpleExpr, scalar: &Scalar) -> Condition {
    Condition::any()
        .add(typed_column_condition(scalar, CmpOp::Equal))
        .add(carries_operand(column, StructuredTarget::AnyValue, scalar))
}

/// Compile a range operand (`==10..20` / `!=10..20`) against the typed columns.
///
/// A range is two ordering comparisons, which JSONB containment cannot express,
/// so a language-selected range has nothing to compile here and is left to the
/// payload fallback that is disjoined with this probe.
///
/// The negated form is the total negation of the interval test, for the reason
/// [`equality_condition`] gives: clause 4.9 makes a target whose datatype is not
/// the range's unequal to it, and a comparison the column cannot even evaluate
/// is unknown rather than false.
fn range_condition(target: StructuredTarget<'_>, min: &Scalar, max: &Scalar, membership: SetMembership) -> Condition {
    match target {
        StructuredTarget::AnyValue => match membership {
            SetMembership::Within => within_range(min, max),
            SetMembership::Outside => Condition::all().add(does_not_hold(within_range(min, max))),
        },
        StructuredTarget::TargetLanguage(_) => unsatisfiable(),
    }
}

/// Whether the typed column holding this range's operand kind sits inside the
/// interval, both bounds included (clause 4.9 `Range`).
fn within_range(min: &Scalar, max: &Scalar) -> Condition {
    Condition::all()
        .add(typed_column_condition(min, CmpOp::GreaterEq))
        .add(typed_column_condition(max, CmpOp::LessEq))
}

/// Compile an ordering comparison (`>`/`>=`/`<`/`<=`) against the typed columns
/// of `current_attribute_values`.
fn compile_ordering_on_projection(id: AttributeNameId, op: OrderingOp, rhs: &Scalar, rows: &EntityRowRefs) -> Condition {
    exists_on_projection(id, Condition::all().add(typed_column_condition(rhs, op.as_cmp_op())), rows)
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Compile `~=` / `!~=` (clause 4.9 `patternOp` / `notPatternOp`).
///
/// Both polarities compile *inside* the `EXISTS`, so both require the attribute
/// to be present. Clause 4.9 is explicit that the entity "shall contain the
/// target element" for `notPatternOp`; an earlier compiler wrapped the whole
/// existence test in `NOT (...)`, which matched entities that simply lacked the
/// attribute.
///
/// Only `value_string` is tested. Clause 4.9 requires a Relationship operand to
/// non-match every operator other than equality and inequality, and the pattern
/// operators are defined over String targets — so a Relationship's object, which
/// is the whole content of `value_uri`, is never a pattern target. Probing it
/// made `owner~="urn:Person:.*"` select entities the standard requires it to
/// reject.
///
/// The attribute-aware trigram index `idx_cav_attr_string_trgm` on
/// `(attribute_name_id int4_ops, value_string gin_trgm_ops)` can accelerate
/// POSIX `~` (since PostgreSQL 9.3) whenever the pattern yields extractable
/// trigrams — `urn:ngsi-ld:.*` does, an anchored `^A.*` does not — and it
/// intersects those trigrams with the attribute key, so a pattern common
/// across attributes yields candidates only from the one probed. When no
/// trigram is extractable the probe is still narrowed by the
/// `attribute_name_id` prefix of `idx_cav_attr_string` before any regex is
/// evaluated.
fn compile_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match local_target(path) {
        LocalTarget::OwnValue => (),
        // A regex is not a containment test, so the projection cannot answer a
        // pattern against one member of a structure; the payload can reach it.
        LocalTarget::TrailingMember(_) | LocalTarget::PayloadMember => return fallback_pattern(path, pattern, polarity, rows),
    }
    let Some(id) = names.attributes.get(&path.attr) else {
        return unsatisfiable();
    };

    let value = Condition::all().add(pattern_predicate(
        probe_column(CurrentAttributeValues::ValueString),
        polarity,
        SeaExpr::val(pattern.as_str()),
    ));
    exists_on_projection(id, value, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dictionary::resolved_names::ResolvedNames, entity_row_refs::DocumentsRef, entity_type_id::EntityTypeId};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::parser;
    use andromeda_postgresql_schema::current::Entities;
    use sea_query::{Alias, PostgresQueryBuilder, Query, Value};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::from_pairs([
                ("https://example.com/Building".parse().expect("valid type name"), EntityTypeId::from(31)),
                ("https://example.com/Store".parse().expect("valid type name"), EntityTypeId::from(32)),
            ]),
        }
    }

    fn attribute_names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameId::from(7)),
            ("https://example.com/brandName".parse().expect("valid attribute name"), AttributeNameId::from(8)),
            ("https://example.com/active".parse().expect("valid attribute name"), AttributeNameId::from(9)),
            ("https://example.com/observed".parse().expect("valid attribute name"), AttributeNameId::from(10)),
            ("https://example.com/d".parse().expect("valid attribute name"), AttributeNameId::from(11)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameId::from(12),
            ),
            ("https://example.com/color".parse().expect("valid attribute name"), AttributeNameId::from(13)),
            ("https://example.com/name".parse().expect("valid attribute name"), AttributeNameId::from(14)),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameId::from(15)),
            ("https://example.com/a".parse().expect("valid attribute name"), AttributeNameId::from(16)),
            ("https://example.com/b".parse().expect("valid attribute name"), AttributeNameId::from(17)),
            ("https://example.com/c".parse().expect("valid attribute name"), AttributeNameId::from(18)),
            (
                "https://example.com/isParkedIn".parse().expect("valid attribute name"),
                AttributeNameId::from(19),
            ),
            ("https://example.com/category".parse().expect("valid attribute name"), AttributeNameId::from(20)),
            ("https://example.com/outer".parse().expect("valid attribute name"), AttributeNameId::from(21)),
            ("https://example.com/inner".parse().expect("valid attribute name"), AttributeNameId::from(22)),
            ("https://example.com/zone".parse().expect("valid attribute name"), AttributeNameId::from(23)),
        ])
    }

    /// Render a compiled condition the way a real query would, so the assertions
    /// see the final placeholder numbering.
    fn render(q: &str) -> (String, Vec<Value>) {
        render_with(q, &names(), &EntityRowRefs::unaliased())
    }

    fn render_with(q: &str, names: &QueryNames, rows: &EntityRowRefs) -> (String, Vec<Value>) {
        let expr = parser::parse(q).expect("parse q");
        let condition = compile(&expr, rows, names);
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

    // -- projection-backed comparisons (clause 4.9) --------------------------

    #[test]
    fn number_comparison_probes_typed_number_column() {
        let (sql, binds) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"current_attribute_values\""), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(7))), "attribute id binds as int4: {binds:?}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "50"),
            "numeric literal binds as NUMERIC: {binds:?}"
        );
    }

    #[test]
    fn a_number_comparison_binds_numeric_without_a_double_conversion() {
        let (sql, binds) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"value_number\" > $2"), "{sql}");
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
        assert!(sql.contains("\"cav\".\"value_string\" ~"), "{sql}");
        assert!(!sql.contains("value_uri"), "a pattern must not test relationship objects: {sql}");
    }

    #[test]
    fn a_negated_pattern_does_not_reach_relationship_objects_either() {
        let (sql, _) = render("\"https://example.com/brandName\"!~=\"Merc.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" !~"), "{sql}");
        assert!(!sql.contains("value_uri"), "{sql}");
    }

    #[test]
    fn equality_covers_property_values_and_relationship_objects() {
        for expression in [
            "\"https://example.com/brandName\"==\"Mercedes\"",
            "\"https://example.com/brandName\"!=\"Mercedes\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains("\"cav\".\"value_string\" "), "{expression}: {sql}");
            assert!(
                sql.contains("\"cav\".\"value_uri\" "),
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
            assert!(sql.contains("\"cav\".\"value_string\" "), "{expression}: {sql}");
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
        assert!(sql.contains("\"cav\".\"value_string\" = "), "the inner condition is the equality one: {sql}");
        assert!(sql.contains("\"cav\".\"value_uri\" = "), "{sql}");
        assert!(sql.contains("\"cav\".\"value_object\" @> "), "{sql}");
        assert!(!sql.contains(" != "), "no per-column inequality survives: {sql}");
    }

    /// The equality operator keeps the plain comparison: clause 4.9 states the
    /// datatype rule the other way for it — "If there is no equality between the
    /// target value data type and the Query Term value data type, then it shall
    /// be considered as not matching" — which is what an unknown already means.
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
        assert!(sql.contains("\"cav\".\"value_number\" >= "), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <= "), "{sql}");
    }

    /// A language-selected inequality keeps its own compilation: clause 4.9
    /// requires the entity to contain the target element, which for that path is
    /// the key-value pair itself, so a `languageMap` without the key is not a
    /// match and neither is a scalar-valued attribute of the same name.
    #[test]
    fn a_language_selected_inequality_still_requires_the_key_rather_than_negating_totally() {
        let (sql, _) = render("\"https://example.com/color\"[en]!=\"red\"");
        assert!(sql.contains("jsonb_build_object('@language', $2::text)"), "the key guard: {sql}");
        assert!(
            !sql.contains("\"cav\".\"value_string\" = "),
            "no typed column answers a language-selected path: {sql}"
        );
    }

    #[test]
    fn no_probe_guards_against_a_stored_deletion_marker() {
        // A deleted attribute is an absent row in the projection, so there is
        // nothing to exclude and no sentinel value a client could collide with.
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(!sql.contains("IS DISTINCT FROM"), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:null"), "{sql}");
    }

    #[test]
    fn no_probe_filters_on_the_value_kind_code() {
        // The projection's check constraint already pins the kind to the
        // populated column, and a `value_kind = k` term would turn the NULL a
        // wrong-kind row yields into FALSE, which a negated predicate then
        // reads as a match.
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert!(!sql.contains("value_kind"), "{sql}");
    }

    #[test]
    fn boolean_comparison_binds_native_boolean() {
        let (sql, binds) = render("\"https://example.com/active\"==true");
        assert!(sql.contains("\"cav\".\"value_bool\" ="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, Value::Bool(Some(true)))), "{binds:?}");
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let (sql, binds) = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert!(sql.contains("\"cav\".\"value_datetime\" >="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, Value::ChronoDateTimeUtc(_))), "{binds:?}");
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_projection_stores_it_that_way() {
        let (sql, _) = render("\"https://example.com/d\"==2024-01-01");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(!sql.contains("value_datetime"), "only full date-times reach value_datetime: {sql}");
    }

    #[test]
    fn a_time_operand_binds_the_canonical_form_the_write_path_stores() {
        // Clause 4.6.3 admits any offset and any fractional width, and the write
        // path stores one canonical rendering of them. The probe has to bind that
        // same rendering or an offset-bearing operand matches nothing.
        let (_, binds) = render("\"https://example.com/d\"==\"13:00:00+01:00\"");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "12:00:00.000000Z")),
            "{binds:?}"
        );
    }

    #[test]
    fn a_time_ordering_comparison_binds_the_canonical_form_too() {
        // The stored column is byte-wise collated, so the comparison is
        // chronological exactly because both sides are the fixed-width form.
        let (sql, binds) = render("\"https://example.com/d\">\"09:30:00.5Z\"");
        assert!(sql.contains("\"cav\".\"value_string\" >"), "{sql}");
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
        assert!(sql.contains("\"cav\".\"value_number\" >="), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <="), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(12))));
        assert!(matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "10"));
        assert!(matches!(&binds[2], Value::BigDecimal(Some(value)) if value.to_string() == "20"));
    }

    #[test]
    fn excluded_range_requires_the_attribute_to_exist() {
        let (sql, _) = render("\"https://example.com/temperature\"!=10..20");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <"), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
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
        assert!(sql.contains("current_attribute_values"), "{sql}");
        assert!(!sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "9007199254740993"),
            "{binds:?}"
        );
    }

    #[test]
    fn exact_fractions_use_the_projection() {
        let (sql, _) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
    }

    // -- patterns ------------------------------------------------------------

    #[test]
    fn pattern_match_uses_posix_regex_on_the_string_column() {
        let (sql, _) = render("\"https://example.com/name\"~=\"^A.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" ~"), "{sql}");
    }

    #[test]
    fn negated_pattern_sits_inside_the_probe_so_missing_attributes_do_not_match() {
        let (sql, _) = render("\"https://example.com/name\"!~=\"^A.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" !~"), "{sql}");
        assert!(
            !sql.contains("NOT (EXISTS") && !sql.contains("NOT EXISTS"),
            "clause 4.9 requires the target element to be present: {sql}"
        );
    }

    // -- structured target values (clause 4.9) -------------------------------

    /// Clause 4.9: with no target language specified, a `LanguageProperty`'s
    /// target value is "a value from any of the key-value pairs held within the
    /// languageMap", and a `ListProperty`'s is "the valueList array" — both of
    /// which the write path projects into `value_object`, so an equality Query
    /// Term has to reach that column and not only the typed ones.
    #[test]
    fn an_unqualified_equality_reaches_the_structured_value_column() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\"");
        assert!(sql.contains("\"cav\".\"value_string\" = "), "{sql}");
        assert!(sql.contains("\"cav\".\"value_object\" @> jsonb_build_array($"), "{sql}");
        assert!(
            sql.contains("\"cav\".\"value_object\" @> jsonb_build_array(jsonb_build_object('@value', $"),
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
        assert!(sql.contains("\"cav\".\"value_object\" @> jsonb_build_array($"), "{sql}");
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
    /// that or a compound value member, so both are compiled.
    #[test]
    fn a_target_language_compiles_a_language_map_probe_beside_the_payload_reading() {
        let (sql, _) = render("\"https://example.com/color\"[en]==\"red\"");
        assert!(
            sql.contains("jsonb_build_object('@language', $2::text, '@value', $3::text)"),
            "the languageMap reading: {sql}"
        );
        assert!(sql.contains("jsonb_path_query_first"), "the compound-value reading: {sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    /// A language-selected path addresses a `languageMap` key. A scalar-valued
    /// attribute of the same name has no such key, so clause 4.9's target
    /// element does not exist for it and neither polarity may consult its typed
    /// column — `color[en]!=5` must not select a plain numeric Property.
    #[test]
    fn a_target_language_never_consults_the_typed_scalar_columns() {
        for expression in ["\"https://example.com/color\"[en]==5", "\"https://example.com/color\"[en]!=5"] {
            let (sql, _) = render(expression);
            assert!(!sql.contains("\"cav\".\"value_number\""), "{expression}: {sql}");
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
        assert!(sql.contains("\"cav\".\"value_object\" @> "), "the structured readings are inside it: {sql}");
        assert!(sql.contains("\"cav\".\"value_string\" = "), "so are the typed columns: {sql}");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(!sql.contains("NOT (EXISTS"), "{sql}");
    }

    /// Clause 4.9 admits a value list against every target: "The target value
    /// includes any of the Query Term values, if the target value is an array".
    #[test]
    fn a_value_list_reaches_the_structured_value_column_once_per_member() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_eq!(sql.matches("\"cav\".\"value_object\" @> ").count(), 4, "two shapes per member: {sql}");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not one per member: {sql}");
    }

    /// The `unequal` complement of the value list: none of the members may be
    /// included in the structured target value.
    #[test]
    fn a_negated_value_list_denies_the_structured_value_column_for_every_member() {
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert_eq!(sql.matches("\"cav\".\"value_object\" @> ").count(), 4, "two element shapes per member: {sql}");
        assert_eq!(sql.matches("NOT COALESCE(").count(), 2, "one negation per member: {sql}");
    }

    /// Clause 4.9 requires a matching Entity to contain the target element,
    /// which for a language-selected path is the key-value pair itself.
    #[test]
    fn a_bare_existence_of_a_target_language_probes_the_language_key() {
        let (sql, _) = render("\"https://example.com/color\"[en]");
        assert!(sql.contains("jsonb_build_object('@language', $2::text)"), "{sql}");
        assert!(!sql.contains("'@value'"), "existence says nothing about the value: {sql}");
        assert!(sql.contains("jsonb_path_exists"), "the compound-member reading is compiled beside it: {sql}");
    }

    /// Clause 4.9's ordering and pattern operators are not containment tests,
    /// so nothing probes the structured column under them; a range is two
    /// ordering comparisons and inherits that.
    #[test]
    fn ordering_pattern_and_range_operators_do_not_probe_the_structured_value_column() {
        for expression in [
            "\"https://example.com/color\">\"red\"",
            "\"https://example.com/color\"~=\"re.*\"",
            "\"https://example.com/color\"!~=\"re.*\"",
            "\"https://example.com/temperature\"==10..20",
            "\"https://example.com/temperature\"!=10..20",
        ] {
            let (sql, _) = render(expression);
            assert!(!sql.contains("value_object"), "{expression}: {sql}");
        }
    }

    // -- unknown attributes --------------------------------------------------

    #[test]
    fn unknown_attribute_iri_compiles_to_a_constant_false() {
        let (sql, _) = render_with(&format!("\"{}\">1", "https://example.com/never-written"), &names(), &EntityRowRefs::unaliased());
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "no probe is emitted when no row can exist: {sql}");
    }

    // -- existence -----------------------------------------------------------

    #[test]
    fn bare_existence_is_the_presence_of_a_projection_row() {
        // A deleted attribute leaves no row, so presence needs no second probe
        // and no tombstone test.
        let (sql, binds) = render(&quoted(SPEED));
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $1 AND "cav"."entity_key" = "entities"."entity_key")"#,
            )
        );
        assert!(matches!(binds[0], Value::Int(Some(7))), "{binds:?}");
    }

    #[test]
    fn unknown_attribute_existence_compiles_to_a_constant_false() {
        let (sql, _) = render("\"https://example.com/never-written\"");
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
    }

    // -- boolean structure ---------------------------------------------------

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

    // -- correlation ---------------------------------------------------------

    #[test]
    fn a_correlated_statement_probes_against_the_entity_key_of_its_own_alias() {
        let (sql, _) = render_with(
            &format!("{}>50", quoted(SPEED)),
            &names(),
            &EntityRowRefs::aliased(Alias::new("ce"), Alias::new("cs"), DocumentsRef::ByKeyLookup),
        );
        assert!(sql.contains("\"cav\".\"entity_key\" = \"ce\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_surrogate_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("\"entities\".\"id\""), "the URI is not the correlation column: {sql}");
    }

    // -- attribute collection ------------------------------------------------

    // -- linked-entity traversal (clause 4.9) --------------------------------

    const IS_PARKED_IN: &str = "https://example.com/isParkedIn";
    const CATEGORY: &str = "https://example.com/category";

    #[test]
    fn a_single_hop_probes_the_relationship_and_joins_the_entity_it_names() {
        let (sql, binds) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attribute_values\" AS \"cav_ln1\""), "{sql}");
        assert!(
            sql.contains("JOIN \"entities\" AS \"le1\" ON \"le1\".\"id\" = \"cav_ln1\".\"value_uri\""),
            "the hop resolves the object through the projection, not the payload: {sql}"
        );
        assert!(sql.contains("\"cav_ln1\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("entity_search"), "a hop without a type guard reads no search row: {sql}");
        // The inner comparison is correlated against the linked entity's key,
        // which is the row the join on its URI landed on.
        assert!(sql.contains("\"cav\".\"entity_key\" = \"le1\".\"entity_key\""), "{sql}");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(
            matches!(binds[0], Value::Int(Some(19))),
            "the relationship's dictionary key binds first: {binds:?}"
        );
        assert!(matches!(binds[1], Value::Int(Some(20))), "then the inner attribute's: {binds:?}");
    }

    #[test]
    fn a_hop_guard_tests_the_linked_entitys_types_for_any_of_its_alternatives() {
        // Clause 4.9 lists the guard's types as alternatives, so one membership
        // test against `entities.type_ids` per resolved type, disjoined.
        let (sql, binds) = render(&format!(
            "{}{{{},{}:{}==\"x\"}}",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Store"),
            quoted(CATEGORY)
        ));
        // The whole statement is pinned, not a substring: the guard renders as a
        // bare `a OR b`, and without parentheses around it the second disjunct
        // escapes the correlation, the attribute-name test and the tombstone
        // guard — the probe would then match whenever any entity in the tenant
        // carried the second type. A `contains` assertion cannot see that.
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav_ln1" "#,
                r#"JOIN "entities" AS "le1" ON "le1"."id" = "cav_ln1"."value_uri" "#,
                r#"JOIN "entity_search" AS "ls1" ON "ls1"."entity_key" = "le1"."entity_key" "#,
                r#"WHERE "cav_ln1"."entity_key" = "entities"."entity_key" "#,
                r#"AND "cav_ln1"."attribute_name_id" = $1 "#,
                r#"AND (("ls1"."type_ids" @> (ARRAY[31])) OR ("ls1"."type_ids" @> (ARRAY[32]))) "#,
                r#"AND EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $2 AND ("cav"."value_string" = $3 OR "cav"."value_uri" = $4 "#,
                r#"OR (("cav"."value_object" @> jsonb_build_array($5::text)) "#,
                r#"OR ("cav"."value_object" @> jsonb_build_array(jsonb_build_object('@value', $6::text))))) "#,
                r#"AND "cav"."entity_key" = "le1"."entity_key"))"#,
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
            "{}{{{},{}:{}==\"x\"}}",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Warehouse"),
            quoted(CATEGORY)
        ));
        assert!(sql.contains(r#"(("ls1"."type_ids" @> (ARRAY[31])) OR (FALSE))"#), "{sql}");
    }

    #[test]
    fn an_unguarded_hop_emits_no_type_test() {
        let (sql, _) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(!sql.contains("\"type_ids\""), "no guard was written: {sql}");
    }

    #[test]
    fn nested_hops_number_their_aliases_by_depth() {
        let (sql, _) = render(&format!(
            "{}{{{}{{{}==\"north\"}}}}",
            quoted("https://example.com/outer"),
            quoted("https://example.com/inner"),
            quoted("https://example.com/zone")
        ));
        for alias in ["cav_ln1", "le1", "cav_ln2", "le2"] {
            assert!(sql.contains(&format!("AS \"{alias}\"")), "{alias} missing from {sql}");
        }
        assert!(
            sql.contains("\"cav_ln2\".\"entity_key\" = \"le1\".\"entity_key\""),
            "the second hop starts at the first's target: {sql}"
        );
        assert!(sql.contains("\"cav\".\"entity_key\" = \"le2\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn a_bare_existence_through_a_hop_probes_the_projection_of_the_linked_entity() {
        let (sql, _) = render(&format!("{}{{{}}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attribute_values\" AS \"cav_ln1\""), "{sql}");
        assert!(
            sql.contains("\"cav\".\"entity_key\" = \"le1\".\"entity_key\""),
            "existence resolves against the entity the hop landed on: {sql}"
        );
    }

    #[test]
    fn an_unresolved_relationship_name_makes_the_traversal_unsatisfiable() {
        // No `attribute_names` row means no instance of the Relationship was
        // ever written, so no entity is reachable through it.
        let (sql, _) = render(&format!("{}{{{}==\"x\"}}", quoted("https://example.com/never-written"), quoted(CATEGORY)));
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("cav_ln1"), "no probe is emitted when no row can exist: {sql}");
    }
}
