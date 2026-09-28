//! One key of an `ORDER BY`, held apart from the statement it orders.
//!
//! A statement that orders its rows in one relation and reads them back
//! through another states the same order twice: once over the expressions and
//! once over the columns those expressions were projected under. The key is
//! therefore a value -- an expression beside the order it runs in -- so each
//! statement level applies it to whichever reference it knows the key by.

use crate::entity_ordering::sql_order;
use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
use sea_query::{NullOrdering, SelectStatement, SimpleExpr};

/// Where a key places the rows that have no value for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullPlacement {
    /// PostgreSQL's own placement: last ascending, first descending.
    Unstated,
    /// Last in either direction, as clause 4.23.2 pins an absent value.
    Last,
}

/// The order one key runs in: its direction and where its nulls go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyOrder {
    /// The direction the key sorts in.
    direction: SortDirection,
    /// Where the rows without a value go.
    nulls: NullPlacement,
}

impl KeyOrder {
    /// The order `direction` with nulls placed as `nulls` says.
    #[must_use]
    pub const fn new(direction: SortDirection, nulls: NullPlacement) -> KeyOrder {
        KeyOrder { direction, nulls }
    }

    /// Order `select` by `key` in this order, after every key already added.
    pub fn order_by(self, select: &mut SelectStatement, key: SimpleExpr) {
        match self.nulls {
            NullPlacement::Unstated => select.order_by_expr(key, sql_order(self.direction)),
            NullPlacement::Last => select.order_by_expr_with_nulls(key, sql_order(self.direction), NullOrdering::Last),
        };
    }
}

/// A sort key: the expression compared and the order it is compared in.
#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    /// The expression the rows are compared on.
    expression: SimpleExpr,
    /// The order the comparison runs in.
    order: KeyOrder,
}

impl SortKey {
    /// The key comparing rows on `expression` in `order`.
    #[must_use]
    pub const fn new(expression: SimpleExpr, order: KeyOrder) -> SortKey {
        SortKey { expression, order }
    }

    /// Order `select` by this key's expression, after every key already added.
    pub fn order_by(self, select: &mut SelectStatement) {
        self.order.order_by(select, self.expression);
    }

    /// The expression and its order, for a statement that projects the
    /// expression and orders by the projected column instead.
    #[must_use]
    pub fn into_parts(self) -> (SimpleExpr, KeyOrder) {
        (self.expression, self.order)
    }
}

#[cfg(test)]
mod tests {
    use crate::sort_key::{KeyOrder, NullPlacement, SortKey};
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn ordered(key: SortKey) -> String {
        let mut select = Query::select();
        select.expr(Expr::col(Alias::new("id")));
        key.order_by(&mut select);
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_unstated_null_placement_leaves_it_to_the_direction() {
        let key = SortKey::new(Expr::col(Alias::new("id")), KeyOrder::new(SortDirection::Descending, NullPlacement::Unstated));
        assert_eq!(ordered(key), r#"SELECT "id" ORDER BY "id" DESC"#);
    }

    #[test]
    fn a_last_null_placement_is_written_in_either_direction() {
        for (direction, spelled) in [(SortDirection::Ascending, "ASC"), (SortDirection::Descending, "DESC")] {
            let key = SortKey::new(Expr::col(Alias::new("id")), KeyOrder::new(direction, NullPlacement::Last));
            assert_eq!(ordered(key), format!(r#"SELECT "id" ORDER BY "id" {spelled} NULLS LAST"#));
        }
    }

    /// A key taken apart orders a projected column exactly as it ordered the
    /// expression, so an outer statement restates the inner one's order.
    #[test]
    fn a_key_taken_apart_orders_another_reference_the_same_way() {
        let (expression, order) = SortKey::new(Expr::col(Alias::new("id")), KeyOrder::new(SortDirection::Ascending, NullPlacement::Last)).into_parts();
        let mut select = Query::select();
        select.expr_as(expression, Alias::new("sort_0"));
        order.order_by(&mut select, Expr::col((Alias::new("page"), Alias::new("sort_0"))));
        assert_eq!(
            select.to_string(PostgresQueryBuilder),
            r#"SELECT "id" AS "sort_0" ORDER BY "page"."sort_0" ASC NULLS LAST"#
        );
    }
}
