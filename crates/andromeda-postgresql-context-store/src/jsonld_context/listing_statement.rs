//! The clause 5.13.3.4 listing of the JSON-LD context store.
//!
//! A page of stored Contexts, optionally of one kind, and the point lookup
//! that reads the same full projection.

use crate::jsonld_context::statement::identity_matches;
use andromeda_context_registry::detail_level::ContextDetailLevel;
use andromeda_ngsi_ld_query::{page_window::PageWindow, read_order::ReadOrder};
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_postgresql_query::page_window::apply_identifier_window;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The full metadata projection, aliased to the metadata row's field names.
///
/// `id` answers under `local_id`, so the row decodes the same whether it came
/// from the list or the point lookup.
fn metadata_columns(statement: &mut SelectStatement) {
    statement
        .expr_as(Expr::col(JsonldContexts::Id), Alias::new(LOCAL_ID))
        .column(JsonldContexts::Url)
        .column(JsonldContexts::Kind)
        .column(JsonldContexts::CreatedAt)
        .column(JsonldContexts::ExpiresAt)
        .column(JsonldContexts::LastUsage)
        .column(JsonldContexts::NumberOfHits)
        .column(JsonldContexts::ExtraInfo)
        .from(JsonldContexts::Table);
}

/// A `kind = $k` filter on the bound kind code.
fn kind_filter(kind: JsonldContextKindCode) -> SimpleExpr {
    Expr::col(JsonldContexts::Kind).eq(kind.code())
}

/// The alias the listed rows are read under.
const LISTED: &str = "listed";

/// The column every listed row is keyed and ordered by.
const LOCAL_ID: &str = "local_id";

/// The stored rows of the listing in the projection `detail_level` names,
/// keyed by `local_id`.
fn stored_projection(detail_level: ContextDetailLevel) -> SelectStatement {
    let mut statement = Query::select();
    match detail_level {
        ContextDetailLevel::Full => metadata_columns(&mut statement),
        ContextDetailLevel::Summary => {
            statement
                .expr_as(Expr::col(JsonldContexts::Id), Alias::new(LOCAL_ID))
                .column(JsonldContexts::Url)
                .from(JsonldContexts::Table);
        }
    }
    statement
}

/// The page `window` selects of the stored Contexts, optionally of one kind,
/// ordered by local identifier, and the order its rows are read in
/// (clause 5.5.9).
#[must_use]
pub fn listing_page_query(detail_level: ContextDetailLevel, kind: Option<JsonldContextKindCode>, window: &PageWindow) -> (SelectStatement, ReadOrder) {
    let mut listed = stored_projection(detail_level);
    if let Some(kind) = kind {
        listed.and_where(kind_filter(kind));
    }
    let mut page = Query::select();
    page.column(Asterisk).from_subquery(listed, Alias::new(LISTED));
    let read_order = apply_identifier_window(&mut page, Expr::col((Alias::new(LISTED), Alias::new(LOCAL_ID))), window);
    (page, read_order)
}

/// The full metadata of the Context identified by `identifier`.
#[must_use]
pub fn metadata_by_identifier_query(identifier: &str) -> SelectStatement {
    let mut statement = Query::select();
    metadata_columns(&mut statement);
    statement.cond_where(identity_matches(identifier));
    statement.clone()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::jsonld_context::listing_statement::listing_page_query;
    use andromeda_context_registry::detail_level::ContextDetailLevel;
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// The listing's SQL and read order for `window`.
    fn listing(detail_level: ContextDetailLevel, kind: Option<JsonldContextKindCode>, window: &PageWindow) -> (String, ReadOrder) {
        let (statement, read_order) = listing_page_query(detail_level, kind, window);
        (statement.to_string(PostgresQueryBuilder), read_order)
    }

    #[test]
    fn the_metadata_projection_aliases_id_and_reads_the_kind_code() {
        let (sql, _) = listing(ContextDetailLevel::Full, None, &PageWindow::offset(50, 0));
        assert!(sql.contains("\"id\" AS \"local_id\""), "{sql}");
        assert!(sql.contains("\"kind\","), "{sql}");
        assert!(!sql.contains("::text"), "{sql}");
    }

    /// Clause 5.5.9.2: an offset page is cut from the local-identifier order.
    #[test]
    fn an_offset_page_orders_by_local_id_with_limit_and_offset() {
        let (sql, read_order) = listing(ContextDetailLevel::Summary, None, &PageWindow::offset(10, 30));
        assert!(sql.contains("ORDER BY \"listed\".\"local_id\" ASC LIMIT 10 OFFSET 30"), "{sql}");
        assert_eq!(read_order, ReadOrder::Ascending);
    }

    #[test]
    fn a_kind_filter_compares_the_kind_code() {
        let (sql, _) = listing(ContextDetailLevel::Summary, Some(JsonldContextKindCode::Hosted), &PageWindow::offset(10, 0));
        assert!(sql.contains("\"kind\" = 2"), "{sql}");
    }

    /// Clause 5.5.9.1: a forward cursor continues past its boundary.
    #[test]
    fn a_forward_cursor_pages_past_its_boundary() {
        let (sql, read_order) = listing(
            ContextDetailLevel::Summary,
            None,
            &PageWindow::after(10, "urn:a".parse().expect("an identifier")),
        );
        assert!(sql.contains("\"listed\".\"local_id\" > 'urn:a'"), "{sql}");
        assert_eq!(read_order, ReadOrder::Ascending);
    }

    /// A backward cursor reads the page before its boundary from that end.
    #[test]
    fn a_backward_cursor_reads_descending_from_its_boundary() {
        let window = PageWindow {
            limit: 10,
            start: PageStart::Cursor(Some(PageCursor::Before("urn:m".parse().expect("an identifier")))),
        };
        let (sql, read_order) = listing(ContextDetailLevel::Summary, None, &window);
        assert!(sql.contains("\"listed\".\"local_id\" < 'urn:m'"), "{sql}");
        assert!(sql.contains("ORDER BY \"listed\".\"local_id\" DESC LIMIT 10"), "{sql}");
        assert_eq!(read_order, ReadOrder::Descending);
    }

    /// Clause 5.13.3.4 lists "@contexts currently stored": the page reads the
    /// stored rows alone, in every projection and kind.
    #[test]
    fn the_listing_reads_the_stored_rows_alone() {
        for detail_level in [ContextDetailLevel::Summary, ContextDetailLevel::Full] {
            for kind in [None, Some(JsonldContextKindCode::Hosted), Some(JsonldContextKindCode::ImplicitlyCreated)] {
                let (sql, _) = listing(detail_level, kind, &PageWindow::offset(10, 0));
                assert!(!sql.contains("UNION"), "the listing reads no other source: {sql}");
            }
        }
    }
}
