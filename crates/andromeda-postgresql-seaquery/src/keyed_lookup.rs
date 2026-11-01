//! A value each row reads by its composite key from a set of key and value
//! pairs, with no join between the rows and the pairs.
//!
//! An `UPDATE … FROM` that takes each row's new value from a staged set joins
//! the two, and the planner picks the join from the row estimates. When both
//! sides look like one row (statistics that still describe a table before a
//! large write, a staged set the planner cannot count), the cheapest plan reads
//! one side again for each row of the other, which costs their product.
//! Aggregated once into a `jsonb` object instead, the pairs are an uncorrelated
//! subquery PostgreSQL evaluates a single time, and each row finds its value by
//! a binary search of the object's keys, so the lookup costs both sides added
//! whatever either estimate says.
//!
//! A key is the text of the row value of its parts, `(a, b, c)::text`, which
//! quotes every part that holds a delimiter, so two distinct keys never share a
//! text. Both sides build it from parts of the same types, so a key matches
//! exactly when its parts do. No part may be NULL.

use crate::{cast::cast, jsonb::jsonb_object_agg, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{Expr, SimpleExpr};

/// The object mapping the key built from `key` to `value`, for every row the
/// enclosing selection reads: `jsonb_object_agg((<key>)::text, <value>)`.
#[must_use]
pub fn keyed_map(key: impl IntoIterator<Item = SimpleExpr>, value: SimpleExpr) -> SimpleExpr {
    jsonb_object_agg(key_text(key), value)
}

/// The value `map`, an object [`keyed_map`] built, holds for the key built
/// from `key`, as `ty`: `((<map>) ->> (<key>)::text)::<ty>`. NULL when `map`
/// holds no such key.
#[must_use]
pub fn keyed_value(map: SimpleExpr, key: impl IntoIterator<Item = SimpleExpr>, ty: PostgresType) -> SimpleExpr {
    cast(map.json_get_text(key_text(key)), ty)
}

/// The text of the row value of `parts`, `(<parts>)::text`.
fn key_text(parts: impl IntoIterator<Item = SimpleExpr>) -> SimpleExpr {
    cast(Expr::tuple(parts), PostgresType::Text)
}

#[cfg(test)]
mod tests {
    use crate::{
        keyed_lookup::{keyed_map, keyed_value},
        pg_type::PostgresType,
        scalar_subquery::scalar_subquery,
    };
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn column(name: &str) -> SimpleExpr {
        Expr::col(Alias::new(name))
    }

    #[test]
    fn the_map_keys_each_value_by_the_text_of_its_row_value() {
        let sql = Query::select()
            .expr(keyed_map([column("entity"), column("name")], column("ordinal")))
            .from(Alias::new("staged"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT jsonb_object_agg(("entity", "name")::text, "ordinal") FROM "staged""#);
    }

    #[test]
    fn a_row_reads_its_value_from_the_map_by_its_own_key() {
        let map = scalar_subquery(
            Query::select()
                .expr(keyed_map([column("entity"), column("name")], column("ordinal")))
                .from(Alias::new("staged"))
                .take(),
        );
        let (sql, values) = Query::select()
            .expr(keyed_value(map, [column("entity_key"), column("name_key")], PostgresType::Integer))
            .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT ((SELECT jsonb_object_agg(("entity", "name")::text, "ordinal") FROM "staged") ->> (("entity_key", "name_key")::text))::integer"#
        );
        assert_eq!(values.0.len(), 0);
    }
}
