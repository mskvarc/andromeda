//! Rendering the names behind a column of dictionary keys, in the column's order.

use andromeda_postgresql_schema::name_dictionary::{AttributeNames, EntityTypeNames};
use andromeda_postgresql_seaquery::pg_array::{any, array_position, array_subquery};
use sea_query::{Expr, ExprTrait, IntoIden, Order, Query, SimpleExpr};

/// The type IRIs a `type_ids` array refers to, in the array's own order: the
/// order the entity declared its types in and the order clause 4.5.1 hands them
/// back in.
#[must_use]
pub fn type_names_of(type_ids: SimpleExpr) -> SimpleExpr {
    names_in_key_order(type_ids, EntityTypeNames::Table, EntityTypeNames::Id, EntityTypeNames::Name)
}

/// The attribute IRIs an array of `attribute_names` keys refers to, in the
/// array's own order: the order a subscriber wrote a subscription's attribute
/// lists in, which a retrieve hands back.
#[must_use]
pub fn attribute_names_of(attribute_name_ids: SimpleExpr) -> SimpleExpr {
    names_in_key_order(attribute_name_ids, AttributeNames::Table, AttributeNames::Id, AttributeNames::Name)
}

/// `ARRAY(SELECT n.name FROM <dictionary> n WHERE n.id = ANY(keys) ORDER BY array_position(keys, n.id))`.
///
/// Rendered as a scalar subquery rather than a join so the statement that
/// projects the names keeps its own shape and cardinality: joining the
/// dictionary would multiply the row by the number of keys. `array_position`
/// keeps the stored order.
fn names_in_key_order(keys: SimpleExpr, table: impl IntoIden + Copy, id: impl IntoIden + Copy, name: impl IntoIden) -> SimpleExpr {
    let mut names = Query::select();
    names
        .column((table, name))
        .from(table)
        .and_where(Expr::col((table, id)).eq(any(keys.clone()))) // clone: the array is named twice in one subquery
        .order_by_expr(array_position(keys, Expr::col((table, id))), Order::Asc);
    array_subquery(&names)
}

#[cfg(test)]
mod tests {
    use crate::dictionary::names_in_key_order::{attribute_names_of, type_names_of};
    use andromeda_postgresql_schema::{current::EntitySearch, subscriptions::Subscriptions};
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn type_names_render_as_an_ordered_array_subquery_over_the_type_dictionary() {
        let sql = Query::select()
            .expr(type_names_of(Expr::col(EntitySearch::TypeIds)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            "SELECT ARRAY(SELECT \"entity_type_names\".\"name\" FROM \"entity_type_names\" WHERE \"entity_type_names\".\"id\" = ANY(\"type_ids\") ORDER BY array_position(\"type_ids\", \"entity_type_names\".\"id\") ASC)"
        );
    }

    #[test]
    fn attribute_names_render_in_the_order_the_keys_were_stored_in() {
        let sql = Query::select()
            .expr(attribute_names_of(Expr::col(Subscriptions::WatchedAttributes)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            "SELECT ARRAY(SELECT \"attribute_names\".\"name\" FROM \"attribute_names\" WHERE \"attribute_names\".\"id\" = ANY(\"watched_attributes\") ORDER BY array_position(\"watched_attributes\", \"attribute_names\".\"id\") ASC)"
        );
    }
}
