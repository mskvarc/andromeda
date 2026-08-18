//! Rendering the type IRIs behind a `type_ids` column.

use andromeda_postgresql_schema::name_dictionary::EntityTypeNames;
use andromeda_postgresql_seaquery::pg_array::{any, array_position, array_subquery};
use sea_query::{Expr, ExprTrait, Order, Query, SimpleExpr};

/// The type IRIs a `type_ids` array refers to, in the array's own order:
/// `ARRAY(SELECT n.name FROM entity_type_names n WHERE n.id = ANY(col) ORDER BY array_position(col, n.id))`.
///
/// Rendered as a scalar subquery rather than a join so the statement that
/// projects or orders on entity types keeps its own shape and cardinality: an
/// entity carries one or two types, and joining the dictionary would multiply
/// its row by that count. `array_position` keeps the stored order, which is
/// the order the entity declared its types in and the order clause 4.5.1 hands
/// them back in.
pub fn type_names_of(type_ids: SimpleExpr) -> SimpleExpr {
    let mut names = Query::select();
    names
        .column((EntityTypeNames::Table, EntityTypeNames::Name))
        .from(EntityTypeNames::Table)
        .and_where(Expr::col((EntityTypeNames::Table, EntityTypeNames::Id)).eq(any(type_ids.clone()))) // clone: the array is named twice in one subquery
        .order_by_expr(array_position(type_ids, Expr::col((EntityTypeNames::Table, EntityTypeNames::Id))), Order::Asc);
    array_subquery(names)
}

#[cfg(test)]
mod tests {
    use crate::entity_type_names::type_names_of;
    use andromeda_postgresql_schema::current::EntitySearch;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn type_names_render_as_an_ordered_array_subquery_over_the_dictionary() {
        let sql = Query::select()
            .expr(type_names_of(Expr::col(EntitySearch::TypeIds)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            "SELECT ARRAY(SELECT \"entity_type_names\".\"name\" FROM \"entity_type_names\" WHERE \"entity_type_names\".\"id\" = ANY(\"type_ids\") ORDER BY array_position(\"type_ids\", \"entity_type_names\".\"id\") ASC)"
        );
    }
}
