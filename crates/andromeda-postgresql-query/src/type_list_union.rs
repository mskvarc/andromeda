//! The union of the type lists an Entity is stated under.
//!
//! Clauses 5.6.11.4 and 5.6.12.4 add the Entity Type names a write states
//! that the Temporal Evolution does not hold yet to its list, so the types a
//! Temporal Evolution holds are the union of every list it was given. Clause
//! 4.5.1 makes that list unordered; it is kept in the order each type first
//! appears, so a list that only grows renders stable, and one list alone
//! keeps its own order.

use andromeda_postgresql_seaquery::{aggregate::array_agg_ordered, pg_array::array_position, unnest::unnest_of};
use sea_query::{Expr, Iden, IntoIden, Order, Query, SelectStatement};

/// A column of the type lists [`type_list_union`] reads, and of the union it
/// yields.
#[derive(Iden, Clone, Copy)]
pub enum TypeListColumn {
    /// `id`, the Entity a list belongs to.
    Id,
    /// `entity_type_name_keys`, the type keys of a list or of the union.
    EntityTypeNameKeys,
    /// `list_rank`, the order of one Entity's lists: a type comes in the
    /// order of the lowest-ranked list holding it. Read, never yielded.
    ListRank,
}

/// One list of `lists`, `stated_list`.
#[derive(Iden, Clone, Copy)]
#[iden = "stated_list"]
struct StatedList;

/// One type key of a list, `stated_key`, both the unnested relation and its
/// one column.
#[derive(Iden, Clone, Copy)]
#[iden = "stated_key"]
struct StatedKey;

/// Each type key of an Entity with the list it first appears in,
/// `first_stated`.
#[derive(Iden, Clone, Copy)]
#[iden = "first_stated"]
struct FirstStated;

/// The position a key takes in the list it first appears in,
/// `key_position`, a column of `first_stated`.
#[derive(Iden, Clone, Copy)]
#[iden = "key_position"]
struct KeyPosition;

/// One row per Entity of `lists`, with the columns `id` and
/// `entity_type_name_keys`: every type key its lists hold, once, in the order
/// of the lowest-ranked list holding it, then of its position there.
///
/// `lists` yields the columns `id`, `entity_type_name_keys` and `list_rank`
/// ([`TypeListColumn`]); an Entity whose lists are all empty yields no row.
#[must_use]
pub fn type_list_union(lists: SelectStatement) -> SelectStatement {
    let keys = || Expr::col((StatedList, TypeListColumn::EntityTypeNameKeys));
    let mut first = Query::select();
    first
        .distinct_on([
            (StatedList.into_iden(), TypeListColumn::Id.into_iden()),
            (StatedKey.into_iden(), StatedKey.into_iden()),
        ])
        .column((StatedList, TypeListColumn::Id))
        .column((StatedKey, StatedKey))
        .column((StatedList, TypeListColumn::ListRank))
        .expr_as(array_position(keys(), Expr::col((StatedKey, StatedKey))), KeyPosition)
        .from_subquery(lists, StatedList)
        .from_function(unnest_of(keys()), StatedKey)
        .order_by((StatedList, TypeListColumn::Id), Order::Asc)
        .order_by((StatedKey, StatedKey), Order::Asc)
        .order_by((StatedList, TypeListColumn::ListRank), Order::Asc);
    let mut union = Query::select();
    union
        .column((FirstStated, TypeListColumn::Id))
        .expr_as(
            array_agg_ordered(
                Expr::col((FirstStated, StatedKey)),
                Expr::tuple([Expr::col((FirstStated, TypeListColumn::ListRank)), Expr::col((FirstStated, KeyPosition))]),
            ),
            TypeListColumn::EntityTypeNameKeys,
        )
        .from_subquery(first, FirstStated)
        .group_by_col((FirstStated, TypeListColumn::Id));
    union
}

#[cfg(test)]
mod tests {
    use crate::type_list_union::{TypeListColumn, type_list_union};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn lists() -> sea_query::SelectStatement {
        Query::select()
            .expr_as(Expr::col(Alias::new("entity_id")), TypeListColumn::Id)
            .expr_as(Expr::col(Alias::new("types")), TypeListColumn::EntityTypeNameKeys)
            .expr_as(Expr::col(Alias::new("intent_key")), TypeListColumn::ListRank)
            .from(Alias::new("lists"))
            .take()
    }

    /// Each key is taken from the lowest-ranked list holding it, with its
    /// position there, and the keys of an Entity aggregate in that order.
    #[test]
    fn each_key_is_kept_once_in_the_order_it_first_appears() {
        let sql = type_list_union(lists()).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "first_stated"."id", array_agg("first_stated"."stated_key" ORDER BY ("first_stated"."list_rank", "first_stated"."key_position")) AS "entity_type_name_keys" FROM "#,
                r#"(SELECT DISTINCT ON ("stated_list"."id", "stated_key"."stated_key") "stated_list"."id", "stated_key"."stated_key", "stated_list"."list_rank", "#,
                r#"array_position("stated_list"."entity_type_name_keys", "stated_key"."stated_key") AS "key_position" "#,
                r#"FROM (SELECT "entity_id" AS "id", "types" AS "entity_type_name_keys", "intent_key" AS "list_rank" FROM "lists") AS "stated_list", "#,
                r#"UNNEST("stated_list"."entity_type_name_keys") AS "stated_key" "#,
                r#"ORDER BY "stated_list"."id" ASC, "stated_key"."stated_key" ASC, "stated_list"."list_rank" ASC) AS "first_stated" "#,
                r#"GROUP BY "first_stated"."id""#
            )
        );
    }
}
