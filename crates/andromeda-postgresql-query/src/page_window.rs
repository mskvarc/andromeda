//! The PostgreSQL spelling of a clause 5.5.9 page window over an identifier.
//!
//! Every identifier-ordered collection the stores page -- entities, temporal
//! entities, registrations, subscriptions -- is cut by the same
//! [`PageWindow`]: an offset is `LIMIT`/`OFFSET`, a cursor is a keyset
//! predicate on the identifier with `LIMIT` and no `OFFSET`. Written once
//! here, so a collection cannot page by a different reading of the window than
//! the in-memory [`PageWindow::cut`] the merged collections are cut by.

use crate::entity_ordering::sql_order;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use sea_query::{Expr, ExprTrait, SelectStatement, SimpleExpr};

/// Bound `select` to the rows `window` selects of a collection ordered by
/// `identifier`, without ordering it, and return the order the rows must be
/// read in.
///
/// For a statement whose order is assembled elsewhere: a requested ordering
/// with the identifier as its tiebreaker, or a sort key restated through an
/// outer statement. A cursor's predicate compares `identifier` exactly as the
/// keyset order does, so the caller orders by the same expression in the
/// returned [`ReadOrder`].
pub fn bound_to_window(select: &mut SelectStatement, identifier: &SimpleExpr, window: &PageWindow) -> ReadOrder {
    match &window.start {
        PageStart::Offset(offset) => {
            select.offset(u64::from(*offset));
        }
        PageStart::Cursor(None | Some(PageCursor::End)) => {}
        // clone: the predicate owns its copy of the key expression.
        PageStart::Cursor(Some(PageCursor::After(boundary))) => {
            select.and_where(identifier.clone().gt(Expr::val(boundary.as_str())));
        }
        // clone: the predicate owns its copy of the key expression.
        PageStart::Cursor(Some(PageCursor::Before(boundary))) => {
            select.and_where(identifier.clone().lt(Expr::val(boundary.as_str())));
        }
    }
    select.limit(u64::from(window.limit));
    window.start.read_order()
}

/// Bound `select` to the rows `window` selects of a collection ordered by
/// `identifier`, order it by the identifier in the direction the window is
/// read in, and return that direction.
///
/// A backward or from-end page is read descending so one index scan from its
/// boundary serves it; the caller presents it ascending with
/// [`ReadOrder::present`].
pub fn apply_identifier_window(select: &mut SelectStatement, identifier: SimpleExpr, window: &PageWindow) -> ReadOrder {
    let order = bound_to_window(select, &identifier, window);
    select.order_by_expr(identifier, sql_order(order.into()));
    order
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::page_window::apply_identifier_window;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    /// The SQL of a select over `items` windowed by `window`, and its read order.
    fn windowed(window: &PageWindow) -> (String, ReadOrder) {
        let mut select = Query::select();
        select.column(Alias::new("id")).from(Alias::new("items"));
        let order = apply_identifier_window(&mut select, Expr::col(Alias::new("id")), window);
        (select.to_string(PostgresQueryBuilder), order)
    }

    fn boundary(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    /// Clause 5.5.9.2: an offset page skips in identifier order.
    #[test]
    fn offset_window_orders_by_id_with_limit_and_offset() {
        let (sql, order) = windowed(&PageWindow::offset(10, 30));
        assert_eq!(sql, r#"SELECT "id" FROM "items" ORDER BY "id" ASC LIMIT 10 OFFSET 30"#);
        assert_eq!(order, ReadOrder::Ascending);
    }

    /// The first page of a cursor walk reads from the start, with no skip.
    #[test]
    fn fresh_cursor_window_orders_by_id_without_offset() {
        let (sql, order) = windowed(&PageWindow {
            limit: 10,
            start: PageStart::Cursor(None),
        });
        assert_eq!(sql, r#"SELECT "id" FROM "items" ORDER BY "id" ASC LIMIT 10"#);
        assert_eq!(order, ReadOrder::Ascending);
    }

    /// Clause 5.5.9.1: a forward cursor reads past its boundary, ascending.
    #[test]
    fn after_window_filters_greater_ascending() {
        let (sql, order) = windowed(&PageWindow::after(10, boundary("urn:e:5")));
        assert_eq!(sql, r#"SELECT "id" FROM "items" WHERE "id" > 'urn:e:5' ORDER BY "id" ASC LIMIT 10"#);
        assert_eq!(order, ReadOrder::Ascending);
    }

    /// A backward cursor reads before its boundary, descending from it.
    #[test]
    fn before_window_filters_less_descending() {
        let (sql, order) = windowed(&PageWindow {
            limit: 10,
            start: PageStart::Cursor(Some(PageCursor::Before(boundary("urn:e:5")))),
        });
        assert_eq!(sql, r#"SELECT "id" FROM "items" WHERE "id" < 'urn:e:5' ORDER BY "id" DESC LIMIT 10"#);
        assert_eq!(order, ReadOrder::Descending);
    }

    /// The from-end cursor reads the final page, descending from the end.
    #[test]
    fn end_window_reads_descending() {
        let (sql, order) = windowed(&PageWindow {
            limit: 10,
            start: PageStart::Cursor(Some(PageCursor::End)),
        });
        assert_eq!(sql, r#"SELECT "id" FROM "items" ORDER BY "id" DESC LIMIT 10"#);
        assert_eq!(order, ReadOrder::Descending);
    }
}
