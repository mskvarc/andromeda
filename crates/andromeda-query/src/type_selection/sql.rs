//! SQL rendering of an expanded type selection expression (clause 4.17).
//!
//! Entity types are stored as a `text[]` column, so every leaf of the
//! expression renders to one array-membership test and the boolean structure
//! renders to `AND` / `OR` around them. The only thing that varies between
//! callers is which relation the column is read from.

use crate::{
    sql::{
        predicate::any_eq,
        schema::{E, Entities},
    },
    type_selection::grammar::TypeExpr,
};
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// Which relation's `types` column a rendering reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypesColumn {
    /// `entities.types`, read unaliased by entity queries, which select from
    /// `entities` directly.
    Entities,
    /// `e.types`, the alias temporal stage 1 left-joins `entities` under.
    AliasedEntities,
}

impl TypesColumn {
    /// The column reference this target reads.
    fn expr(self) -> SimpleExpr {
        match self {
            TypesColumn::Entities => SeaExpr::col(Entities::Types).into(),
            TypesColumn::AliasedEntities => SeaExpr::col((E, Entities::Types)).into(),
        }
    }
}

/// Render a type expression against the given `types` column.
pub fn condition(expr: &TypeExpr, column: TypesColumn) -> Condition {
    match expr {
        TypeExpr::Type(iri) => Condition::all().add(any_eq(SeaExpr::val(iri.as_str()).into(), column.expr())),
        TypeExpr::And(children) => children.iter().fold(Condition::all(), |cond, child| cond.add(condition(child, column))),
        TypeExpr::Or(children) => children.iter().fold(Condition::any(), |cond, child| cond.add(condition(child, column))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;
    use sea_query::{PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_single_type_renders_an_array_membership_test() {
        let sql = render(condition(&TypeExpr::Type(iri("https://example.com/Vehicle")), TypesColumn::Entities));
        assert!(sql.contains("= ANY"), "{sql}");
        assert!(sql.contains("\"types\""), "{sql}");
    }

    #[test]
    fn a_disjunction_renders_or_between_membership_tests() {
        let expr = TypeExpr::Or(vec![TypeExpr::Type(iri("https://example.com/A")), TypeExpr::Type(iri("https://example.com/B"))]);
        let sql = render(condition(&expr, TypesColumn::Entities));
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn a_conjunction_renders_and_between_membership_tests() {
        let expr = TypeExpr::And(vec![TypeExpr::Type(iri("https://example.com/A")), TypeExpr::Type(iri("https://example.com/B"))]);
        let sql = render(condition(&expr, TypesColumn::Entities));
        assert!(sql.contains(" AND "), "{sql}");
    }

    #[test]
    fn a_nested_expression_keeps_its_grouping() {
        let expr = TypeExpr::Or(vec![
            TypeExpr::And(vec![
                TypeExpr::Type(iri("https://example.com/Home")),
                TypeExpr::Type(iri("https://example.com/Vehicle")),
            ]),
            TypeExpr::Type(iri("https://example.com/Motorhome")),
        ]);
        let sql = render(condition(&expr, TypesColumn::Entities));
        assert!(sql.contains(" AND "), "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    /// The two targets differ in one thing only, and this is it. Before the two
    /// renderings were merged, a change to the leaf shape had to be made twice.
    #[test]
    fn the_two_targets_differ_only_in_which_relation_they_read() {
        let expr = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let unaliased = render(condition(&expr, TypesColumn::Entities));
        let aliased = render(condition(&expr, TypesColumn::AliasedEntities));
        assert_eq!(unaliased.replace("\"types\"", "\"e\".\"types\""), aliased);
    }
}
