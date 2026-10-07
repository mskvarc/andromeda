//! SQL rendering of an expanded type selection expression (clause 4.17).
//!
//! Entity types are stored as an `integer[]` of `entity_type_names` keys, so
//! every leaf of the expression renders to one array-containment test on the
//! key the type resolved to, and the boolean structure renders to `AND` /
//! `OR` around them. A type the dictionary has never recorded has no key, and
//! no entity can carry a type that was never interned, so its leaf renders as
//! constant `FALSE`. The only thing that varies between callers is which
//! relation's `entity_type_name_keys` column is read.

use crate::dictionary::resolved_names::ResolvedNames;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_postgresql_seaquery::{literal::false_literal, operator::array_contains_key};
use sea_query::{Condition, SimpleExpr};

/// Render a type expression against the given `entity_type_name_keys` column.
#[must_use]
pub fn condition(expr: &TypeExpr, entity_type_name_keys: &SimpleExpr, types: &ResolvedNames<EntityTypeName>) -> Condition {
    match expr {
        TypeExpr::Wildcard => Condition::all(),
        TypeExpr::Type(iri) => Condition::all().add(match types.get_str(iri.as_str()) {
            Some(id) => array_contains_key(entity_type_name_keys.clone(), id.get()), // clone: each leaf names the same column
            None => false_literal(),
        }),
        TypeExpr::And(children) => children
            .iter()
            .fold(Condition::all(), |cond, child| cond.add(condition(child, entity_type_name_keys, types))),
        TypeExpr::Or(children) => children
            .iter()
            .fold(Condition::any(), |cond, child| cond.add(condition(child, entity_type_name_keys, types))),
    }
}

#[cfg(test)]
mod tests {
    use crate::{dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey, type_predicate::condition};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use andromeda_postgresql_schema::{
        current::{E, Entities, EntitySearch},
        temporal::TemporalEntities,
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, Expr, PostgresQueryBuilder, Query, SimpleExpr};

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

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.17 selects on membership, and the containment spelling over
    /// the resolved key is the one the `entity_type_name_keys` GIN index can answer.
    #[test]
    fn a_known_type_renders_an_array_containment_test_on_its_key() {
        let sql = render(condition(
            &TypeExpr::Type(iri("https://example.com/Vehicle")),
            &unaliased(),
            &known(&[("https://example.com/Vehicle", 7)]),
        ));
        assert!(
            sql.contains("\"entity_type_name_keys\" @> (ARRAY[7])"),
            "the key is inlined for the planner: {sql}"
        );
        assert!(!sql.contains("= ANY"), "{sql}");
        assert!(!sql.contains("Vehicle"), "the type IRI itself never reaches the statement: {sql}");
    }

    /// A type the dictionary never recorded was never written to any entity,
    /// so the leaf is unsatisfiable rather than an unbound probe.
    #[test]
    fn an_unknown_type_renders_false() {
        let sql = render(condition(
            &TypeExpr::Type(iri("https://example.com/Vehicle")),
            &unaliased(),
            &ResolvedNames::default(),
        ));
        assert!(sql.contains("WHERE FALSE"), "{sql}");
    }

    #[test]
    fn a_disjunction_renders_or_between_membership_tests() {
        let expr = TypeExpr::Or(vec![TypeExpr::Type(iri("https://example.com/A")), TypeExpr::Type(iri("https://example.com/B"))]);
        let sql = render(condition(
            &expr,
            &unaliased(),
            &known(&[("https://example.com/A", 1), ("https://example.com/B", 2)]),
        ));
        assert!(sql.contains("(ARRAY[1])) OR ("), "{sql}");
        assert!(sql.contains("(ARRAY[2])"), "{sql}");
    }

    #[test]
    fn a_conjunction_renders_and_between_membership_tests() {
        let expr = TypeExpr::And(vec![TypeExpr::Type(iri("https://example.com/A")), TypeExpr::Type(iri("https://example.com/B"))]);
        let sql = render(condition(
            &expr,
            &unaliased(),
            &known(&[("https://example.com/A", 1), ("https://example.com/B", 2)]),
        ));
        assert!(sql.contains(" AND "), "{sql}");
    }

    /// One unresolved alternative does not poison the others: the disjunction
    /// keeps its resolved arms and only the unknown one is constant `FALSE`.
    #[test]
    fn a_nested_expression_keeps_its_grouping_around_an_unresolved_leaf() {
        let expr = TypeExpr::Or(vec![
            TypeExpr::And(vec![
                TypeExpr::Type(iri("https://example.com/Home")),
                TypeExpr::Type(iri("https://example.com/Vehicle")),
            ]),
            TypeExpr::Type(iri("https://example.com/Motorhome")),
        ]);
        let sql = render(condition(
            &expr,
            &unaliased(),
            &known(&[("https://example.com/Home", 1), ("https://example.com/Vehicle", 2)]),
        ));
        assert!(
            sql.contains("((\"entity_type_name_keys\" @> (ARRAY[1])) AND (\"entity_type_name_keys\" @> (ARRAY[2]))) OR (FALSE)"),
            "{sql}"
        );
    }

    /// The temporal path reads the catalogue's column under its `e` alias.
    #[test]
    fn the_aliased_temporal_column_differs_only_in_which_relation_it_reads() {
        let expr = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let names = known(&[("https://example.com/Vehicle", 7)]);
        let unaliased = render(condition(&expr, &unaliased(), &names));
        let aliased = render(condition(&expr, &Expr::col((E, TemporalEntities::EntityTypeNameKeys)), &names));
        assert_eq!(unaliased.replace("\"entity_type_name_keys\"", "\"e\".\"entity_type_name_keys\""), aliased);
    }
}
