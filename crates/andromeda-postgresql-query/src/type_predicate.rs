//! SQL rendering of an expanded type selection expression (clause 4.17), over
//! the two forms the types of a subject are stored in.
//!
//! An Entity's types are an `integer[]` of `entity_type_names` keys, so every
//! leaf of the expression renders to one array-containment test on the key
//! the type resolved to, and the boolean structure renders to `AND` / `OR`
//! around them. A type the dictionary has never recorded has no key, and no
//! entity can carry a type that was never interned, so its leaf renders as
//! constant `FALSE`. The only thing that varies between callers is which
//! relation's `entity_type_name_keys` column is read.
//!
//! A registration's `EntityInfo` holds its types as a `text[]` of the expanded
//! names it was registered with (clause 5.12), which no dictionary keys, so
//! [`names_condition`] tests the names themselves, the names a group lists
//! directly in one `@>` for a conjunction and one `&&` for a disjunction.

use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
use andromeda_postgresql_seaquery::{literal::false_literal, operator::array_contains_key, pg_array::text_array, pg_operator::PgBinaryOperator};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Condition, SimpleExpr};

/// Render a type selection against the given `entity_type_name_keys` column.
#[must_use]
pub fn condition(expr: &TypeSelection<Expanded>, entity_type_name_keys: &SimpleExpr, types: &ResolvedNames<EntityTypeName>) -> Condition {
    selection_condition(expr, |iri| {
        Condition::all().add(match types.get_str(iri.as_str()) {
            Some(id) => array_contains_key(entity_type_name_keys.clone(), id.get()), // clone: each leaf names the same column
            None => false_literal(),
        })
    })
}

/// The condition of a type selection whose every type is tested by `leaf`:
/// the wildcard admits everything, a conjunction needs all its types and the
/// alternatives are a disjunction. A conjunction of one type and a selection of
/// one alternative stand for themselves, without a group around them.
pub(crate) fn selection_condition(selection: &TypeSelection<Expanded>, leaf: impl Fn(&EntityTypeIri) -> Condition) -> Condition {
    match selection {
        TypeSelection::Wildcard => Condition::all(),
        TypeSelection::Alternatives(alternatives) => alone_or_combined(
            alternatives
                .iter()
                .map(|conjunction| alone_or_combined(conjunction.iter().map(&leaf).collect(), Condition::all()))
                .collect(),
            Condition::any(),
        ),
    }
}

/// The one condition of `conditions` as it is, or all of them added to `group`.
fn alone_or_combined(conditions: Vec<Condition>, group: Condition) -> Condition {
    match <[Condition; 1]>::try_from(conditions) {
        Ok([only]) => only,
        Err(several) => several.into_iter().fold(group, Condition::add),
    }
}

/// Whether the `text[]` of expanded type names `entity_types` satisfies
/// `selection`, or `None` when the selection admits every subject (the
/// wildcard).
///
/// Clause 5.12 matches the type selector against the entity types of an
/// `EntityInfo`: a conjunction needs every named type among them and a
/// disjunction one of them, so the names a conjunction lists directly are one
/// `@>` and those a disjunction lists one `&&`.
#[must_use]
pub fn names_condition(selection: &TypeSelection<Expanded>, entity_types: &SimpleExpr) -> Option<SimpleExpr> {
    let TypeSelection::Alternatives(alternatives) = selection else {
        return None;
    };
    let contains_all = |conjunction: &[EntityTypeIri]| {
        let names: Vec<&str> = conjunction.iter().map(EntityTypeIri::as_str).collect();
        entity_types.clone().array_contains(text_array(&names)) // clone: every group tests the same column
    };
    if let [conjunction] = alternatives.iter().collect::<Vec<_>>().as_slice() {
        return Some(match conjunction {
            [_] => contains_all(conjunction),
            [] | [_, _, ..] => Condition::all().add(contains_all(conjunction)).into(),
        });
    }
    let (single, grouped): (Vec<&[EntityTypeIri]>, Vec<&[EntityTypeIri]>) = alternatives.iter().partition(|conjunction| conjunction.len() == 1);
    let names: Vec<&str> = single.into_iter().flatten().map(EntityTypeIri::as_str).collect();
    let any = grouped
        .into_iter()
        .map(|conjunction| SimpleExpr::from(Condition::all().add(contains_all(conjunction))))
        .fold(Condition::any(), Condition::add);
    let any = if names.is_empty() {
        any
    } else {
        any.add(entity_types.clone().array_overlaps(text_array(&names))) // clone: the group's names test the same column
    };
    Some(any.into())
}

/// Whether the overlap of a subject's names with the names `selection` lists
/// decides the selection exactly: one name, or a disjunction of names.
#[must_use]
pub fn names_decide(selection: &TypeSelection<Expanded>) -> bool {
    match selection {
        TypeSelection::Wildcard => false,
        TypeSelection::Alternatives(alternatives) => alternatives.iter().all(|conjunction| conjunction.len() == 1),
    }
}

#[cfg(test)]
mod tests {
    use crate::type_predicate::{condition, names_condition, names_decide};
    use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
    use andromeda_ngsi_ld_query::{name_stage::Expanded, type_selection::TypeSelection};
    use andromeda_postgresql_schema::current::{E, Entities, EntitySearch};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, Expr, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

    fn iri(value: &str) -> EntityTypeIri {
        value.parse().expect("valid IRI")
    }

    fn alternatives(shape: &[&[&str]]) -> TypeSelection<Expanded> {
        TypeSelection::of_alternatives(shape.iter().map(|conjunction| conjunction.iter().map(|value| iri(value)).collect()).collect())
            .expect("a non-empty shape")
    }

    fn known(pairs: &[(&str, i32)]) -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs(
            pairs
                .iter()
                .map(|(name, id)| (name.parse::<EntityTypeName>().expect("valid type name"), EntityTypeNameKey::from(*id))),
        )
    }

    fn unaliased() -> SimpleExpr {
        Expr::col(EntitySearch::EntityTypeNameKeys)
    }

    fn render(condition: Condition) -> SelectStatement {
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
    }

    /// Clause 4.17 selects on membership, and the containment spelling over
    /// the resolved key is the one the `entity_type_name_keys` GIN index can answer.
    #[test]
    fn a_known_type_renders_an_array_containment_test_on_its_key() {
        assert_statement_snapshot!(
            "type-predicate-a-known-type-renders-an-array-containment-test-on-its-key",
            render(condition(
                &alternatives(&[&["https://example.com/Vehicle"]]),
                &unaliased(),
                &known(&[("https://example.com/Vehicle", 7)]),
            ))
        );
    }

    /// A type the dictionary never recorded was never written to any entity,
    /// so the leaf is unsatisfiable rather than an unbound probe.
    #[test]
    fn an_unknown_type_renders_false() {
        assert_statement_snapshot!(
            "type-predicate-an-unknown-type-renders-false",
            render(condition(
                &alternatives(&[&["https://example.com/Vehicle"]]),
                &unaliased(),
                &ResolvedNames::default(),
            ))
        );
    }

    #[test]
    fn a_disjunction_renders_or_between_membership_tests() {
        let expr = alternatives(&[&["https://example.com/A"], &["https://example.com/B"]]);
        assert_statement_snapshot!(
            "type-predicate-a-disjunction-renders-or-between-membership-tests",
            render(condition(
                &expr,
                &unaliased(),
                &known(&[("https://example.com/A", 1), ("https://example.com/B", 2)]),
            ))
        );
    }

    #[test]
    fn a_conjunction_renders_and_between_membership_tests() {
        let expr = alternatives(&[&["https://example.com/A", "https://example.com/B"]]);
        assert_statement_snapshot!(
            "type-predicate-a-conjunction-renders-and-between-membership-tests",
            render(condition(
                &expr,
                &unaliased(),
                &known(&[("https://example.com/A", 1), ("https://example.com/B", 2)]),
            ))
        );
    }

    /// One unresolved alternative does not poison the others: the disjunction
    /// keeps its resolved arms and only the unknown one is constant `FALSE`.
    #[test]
    fn a_nested_expression_keeps_its_grouping_around_an_unresolved_leaf() {
        let expr = alternatives(&[&["https://example.com/Home", "https://example.com/Vehicle"], &["https://example.com/Motorhome"]]);
        assert_statement_snapshot!(
            "type-predicate-a-nested-expression-keeps-its-grouping-around-an-unresolved-leaf",
            render(condition(
                &expr,
                &unaliased(),
                &known(&[("https://example.com/Home", 1), ("https://example.com/Vehicle", 2)]),
            ))
        );
    }

    /// The temporal path reads the catalogue's column under its `e` alias.
    #[test]
    fn the_aliased_temporal_column_differs_only_in_which_relation_it_reads() {
        let expr = alternatives(&[&["https://example.com/Vehicle"]]);
        let names = known(&[("https://example.com/Vehicle", 7)]);
        let unaliased = render(condition(&expr, &unaliased(), &names));
        let aliased = render(condition(&expr, &Expr::col((E, Entities::EvolutionTypeNameKeys)), &names));
        assert_statement_snapshot!(
            "type-predicate-the-aliased-temporal-column-differs-only-in-which-relation-it-reads-1",
            unaliased
        );
        assert_statement_snapshot!("type-predicate-the-aliased-temporal-column-differs-only-in-which-relation-it-reads-2", aliased);
        assert_eq!(
            unaliased
                .to_string(PostgresQueryBuilder)
                .replace("\"entity_type_name_keys\"", "\"e\".\"evolution_type_name_keys\""),
            aliased.to_string(PostgresQueryBuilder)
        );
    }

    fn names_sql(selection: &TypeSelection<Expanded>) -> Option<String> {
        names_condition(selection, &unaliased()).map(|expr| Query::select().expr(expr).to_string(PostgresQueryBuilder))
    }

    /// Clause 5.12: the registered names are tested directly, one name as a
    /// containment, a conjunction as a group of containment, and the names of
    /// a disjunction in one overlap beside the groups of its conjunctions.
    #[test]
    fn registered_names_are_tested_by_containment_overlap_and_groups() {
        let (a, b, c) = ("https://example.com/A", "https://example.com/B", "https://example.com/C");
        assert_eq!(names_sql(&TypeSelection::Wildcard), None);
        let rendered = |shape: &[&[&str]]| names_sql(&alternatives(shape)).expect("a type selection tests names");
        assert_eq!(rendered(&[&[a]]), r#"SELECT "entity_type_name_keys" @> ARRAY ['https://example.com/A']"#);
        assert_eq!(
            rendered(&[&[a, b]]),
            r#"SELECT "entity_type_name_keys" @> ARRAY ['https://example.com/A','https://example.com/B']"#
        );
        assert_eq!(
            rendered(&[&[a], &[b]]),
            r#"SELECT "entity_type_name_keys" && ARRAY ['https://example.com/A','https://example.com/B']"#
        );
        assert_eq!(
            rendered(&[&[a, b], &[c]]),
            r#"SELECT ("entity_type_name_keys" @> ARRAY ['https://example.com/A','https://example.com/B']) OR ("entity_type_name_keys" && ARRAY ['https://example.com/C'])"#
        );
    }

    /// The overlap with the names a selection lists decides one name and a
    /// disjunction of names, and no conjunction or wildcard.
    #[test]
    fn the_name_overlap_decides_one_name_and_a_disjunction_of_names_alone() {
        let (a, b) = ("https://example.com/A", "https://example.com/B");
        assert!(names_decide(&alternatives(&[&[a]])));
        assert!(names_decide(&alternatives(&[&[a], &[b]])));
        assert!(!names_decide(&alternatives(&[&[a, b]])));
        assert!(!names_decide(&alternatives(&[&[a], &[a, b]])));
        assert!(!names_decide(&TypeSelection::Wildcard));
    }
}
