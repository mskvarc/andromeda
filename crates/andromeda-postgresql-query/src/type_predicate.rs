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

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::expanded_type_selection::ExpandedTypeSelection;
use andromeda_postgresql_seaquery::{literal::false_literal, operator::array_contains_key, pg_array::text_array, pg_operator::PgBinaryOperator};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Condition, SimpleExpr};

/// Render a type selection against the given `entity_type_name_keys` column.
#[must_use]
pub fn condition(expr: &ExpandedTypeSelection, entity_type_name_keys: &SimpleExpr, types: &ResolvedNames<EntityTypeName>) -> Condition {
    match expr {
        ExpandedTypeSelection::Wildcard => Condition::all(),
        ExpandedTypeSelection::Type(iri) => Condition::all().add(match types.get_str(iri.as_str()) {
            Some(id) => array_contains_key(entity_type_name_keys.clone(), id.get()), // clone: each leaf names the same column
            None => false_literal(),
        }),
        ExpandedTypeSelection::And(children) => children
            .iter()
            .fold(Condition::all(), |cond, child| cond.add(condition(child, entity_type_name_keys, types))),
        ExpandedTypeSelection::Or(children) => children
            .iter()
            .fold(Condition::any(), |cond, child| cond.add(condition(child, entity_type_name_keys, types))),
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
pub fn names_condition(selection: &ExpandedTypeSelection, entity_types: &SimpleExpr) -> Option<SimpleExpr> {
    match selection {
        ExpandedTypeSelection::Wildcard => None,
        ExpandedTypeSelection::Type(entity_type) => Some(entity_types.clone().array_contains(text_array(&[entity_type.as_str()]))), // clone: each leaf names the same column
        ExpandedTypeSelection::And(children) => {
            let names = direct_names(children);
            let all = children
                .iter()
                .filter(|child| !matches!(child, ExpandedTypeSelection::Type(_)))
                .filter_map(|child| names_condition(child, entity_types))
                .fold(Condition::all(), Condition::add);
            let all = if names.is_empty() {
                all
            } else {
                all.add(entity_types.clone().array_contains(text_array(&names))) // clone: the group's names test the same column
            };
            Some(all.into())
        }
        ExpandedTypeSelection::Or(children) => {
            let names = direct_names(children);
            let others = children
                .iter()
                .filter(|child| !matches!(child, ExpandedTypeSelection::Type(_)))
                .map(|child| names_condition(child, entity_types))
                .collect::<Option<Vec<_>>>()?;
            let any = others.into_iter().fold(Condition::any(), Condition::add);
            let any = if names.is_empty() {
                any
            } else {
                any.add(entity_types.clone().array_overlaps(text_array(&names))) // clone: the group's names test the same column
            };
            Some(any.into())
        }
    }
}

/// Whether the overlap of a subject's names with the names `selection` lists
/// decides the selection exactly: one name, or a disjunction of names.
#[must_use]
pub fn names_decide(selection: &ExpandedTypeSelection) -> bool {
    match selection {
        ExpandedTypeSelection::Type(_) => true,
        ExpandedTypeSelection::Or(children) => children.iter().all(|child| matches!(child, ExpandedTypeSelection::Type(_))),
        ExpandedTypeSelection::Wildcard | ExpandedTypeSelection::And(_) => false,
    }
}

/// The names `children` list directly, each a leaf of the expression.
fn direct_names(children: &[ExpandedTypeSelection]) -> Vec<&str> {
    children
        .iter()
        .filter_map(|child| match child {
            ExpandedTypeSelection::Type(entity_type) => Some(entity_type.as_str()),
            ExpandedTypeSelection::Wildcard | ExpandedTypeSelection::And(_) | ExpandedTypeSelection::Or(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::type_predicate::{condition, names_decide};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::type_selection::expanded_type_selection::ExpandedTypeSelection;
    use andromeda_postgresql_schema::current::{E, Entities, EntitySearch};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, Expr, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
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
                &ExpandedTypeSelection::Type(iri("https://example.com/Vehicle")),
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
                &ExpandedTypeSelection::Type(iri("https://example.com/Vehicle")),
                &unaliased(),
                &ResolvedNames::default(),
            ))
        );
    }

    #[test]
    fn a_disjunction_renders_or_between_membership_tests() {
        let expr = ExpandedTypeSelection::Or(vec![
            ExpandedTypeSelection::Type(iri("https://example.com/A")),
            ExpandedTypeSelection::Type(iri("https://example.com/B")),
        ]);
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
        let expr = ExpandedTypeSelection::And(vec![
            ExpandedTypeSelection::Type(iri("https://example.com/A")),
            ExpandedTypeSelection::Type(iri("https://example.com/B")),
        ]);
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
        let expr = ExpandedTypeSelection::Or(vec![
            ExpandedTypeSelection::And(vec![
                ExpandedTypeSelection::Type(iri("https://example.com/Home")),
                ExpandedTypeSelection::Type(iri("https://example.com/Vehicle")),
            ]),
            ExpandedTypeSelection::Type(iri("https://example.com/Motorhome")),
        ]);
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
        let expr = ExpandedTypeSelection::Type(iri("https://example.com/Vehicle"));
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

    /// The overlap with the names a selection lists decides one name and a
    /// disjunction of names, and no conjunction, group or wildcard.
    #[test]
    fn the_name_overlap_decides_one_name_and_a_disjunction_of_names_alone() {
        let name = |value: &str| ExpandedTypeSelection::Type(iri(value));
        let (a, b) = ("https://example.com/A", "https://example.com/B");
        assert!(names_decide(&name(a)));
        assert!(names_decide(&ExpandedTypeSelection::Or(vec![name(a), name(b)])));
        assert!(!names_decide(&ExpandedTypeSelection::And(vec![name(a), name(b)])));
        assert!(!names_decide(&ExpandedTypeSelection::Or(vec![
            name(a),
            ExpandedTypeSelection::And(vec![name(a), name(b)])
        ])));
        assert!(!names_decide(&ExpandedTypeSelection::Wildcard));
    }
}
