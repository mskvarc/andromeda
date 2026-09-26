//! Rendering the names behind a column of dictionary keys, in the column's order.

use andromeda_postgresql_schema::name_dictionary::{AttributeNames, EntityTypeNames};
use andromeda_postgresql_seaquery::pg_array::{any, array_position, array_subquery};
use sea_query::{Expr, ExprTrait, IntoIden, Order, Query, SimpleExpr};

/// The type IRIs an `entity_type_name_keys` array refers to, in the array's own
/// order: the order the entity declared its types in and the order clause 4.5.1
/// hands them back in.
#[must_use]
pub fn type_names_of(entity_type_name_keys: SimpleExpr) -> SimpleExpr {
    names_in_key_order(
        entity_type_name_keys,
        EntityTypeNames::Table,
        EntityTypeNames::EntityTypeNameKey,
        EntityTypeNames::Name,
    )
}

/// The attribute IRIs an array of `attribute_names` keys refers to, in the
/// array's own order: the order a subscriber wrote a subscription's attribute
/// lists in, which a retrieve hands back.
#[must_use]
pub fn attribute_names_of(attribute_name_keys: SimpleExpr) -> SimpleExpr {
    names_in_key_order(
        attribute_name_keys,
        AttributeNames::Table,
        AttributeNames::AttributeNameKey,
        AttributeNames::Name,
    )
}

/// `ARRAY(SELECT n.name FROM <dictionary> n WHERE n.<key> = ANY(keys) ORDER BY array_position(keys, n.<key>))`.
///
/// Rendered as a scalar subquery rather than a join so the statement that
/// projects the names keeps its own shape and cardinality: joining the
/// dictionary would multiply the row by the number of keys. `array_position`
/// keeps the stored order.
fn names_in_key_order(keys: SimpleExpr, table: impl IntoIden + Copy, key: impl IntoIden + Copy, name: impl IntoIden) -> SimpleExpr {
    let mut names = Query::select();
    names
        .column((table, name))
        .from(table)
        .and_where(Expr::col((table, key)).eq(any(keys.clone()))) // clone: the array is named twice in one subquery
        .order_by_expr(array_position(keys, Expr::col((table, key))), Order::Asc);
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
            .expr(type_names_of(Expr::col(EntitySearch::EntityTypeNameKeys)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            "SELECT ARRAY(SELECT \"entity_type_names\".\"name\" FROM \"entity_type_names\" WHERE \"entity_type_names\".\"entity_type_name_key\" = ANY(\"entity_type_name_keys\") ORDER BY array_position(\"entity_type_name_keys\", \"entity_type_names\".\"entity_type_name_key\") ASC)"
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
            "SELECT ARRAY(SELECT \"attribute_names\".\"name\" FROM \"attribute_names\" WHERE \"attribute_names\".\"attribute_name_key\" = ANY(\"watched_attributes\") ORDER BY array_position(\"watched_attributes\", \"attribute_names\".\"attribute_name_key\") ASC)"
        );
    }
}
