//! The clause 5.13.3.4 listing of the JSON-LD context store.
//!
//! A page of stored Contexts, optionally of one kind, with the Context the
//! broker serves without storing it as one more row of the same projection,
//! and the point lookup that reads the same full projection.

use crate::jsonld_context::{
    kind_code::stored_kind,
    statement::{identity_matches, json},
};
use andromeda_context_registry::detail_level::ContextDetailLevel;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use andromeda_ngsi_ld_query::{page_window::PageWindow, read_order::ReadOrder};
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_postgresql_query::page_window::apply_identifier_window;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Keyword, Query, SelectStatement, SimpleExpr, UnionType};
use std::num::TryFromIntError;

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

/// The one row the Context the broker serves without storing it reads as, in
/// the projection `detail_level` names, column for column with
/// [`stored_projection`].
///
/// # Errors
///
/// Returns [`TryFromIntError`] when the entry's hit count exceeds the
/// `bigint` column the projection reads it as.
fn embedded_projection(detail_level: ContextDetailLevel, embedded: &ContextMetadata) -> Result<SelectStatement, TryFromIntError> {
    let mut statement = Query::select();
    statement
        .expr_as(Expr::val(embedded.local_id.as_str()), Alias::new(LOCAL_ID))
        .expr_as(Expr::val(embedded.url.as_ref().map(|url| url.as_str().to_owned())), JsonldContexts::Url);
    match detail_level {
        ContextDetailLevel::Full => {
            statement
                .expr_as(Expr::val(stored_kind(&embedded.kind).code()), JsonldContexts::Kind)
                .expr_as(Expr::val(embedded.created_at), JsonldContexts::CreatedAt)
                .expr_as(Expr::val(embedded.expires_at), JsonldContexts::ExpiresAt)
                .expr_as(Expr::val(embedded.last_usage), JsonldContexts::LastUsage)
                .expr_as(Expr::val(i64::try_from(embedded.number_of_hits)?), JsonldContexts::NumberOfHits)
                .expr_as(
                    embedded
                        .extra_info
                        .as_ref()
                        .map_or_else(|| cast(Keyword::Null.into(), PostgresType::Json), json),
                    JsonldContexts::ExtraInfo,
                );
        }
        ContextDetailLevel::Summary => {}
    }
    Ok(statement)
}

/// The page `window` selects of the stored Contexts, optionally of one kind,
/// and of `embedded`, ordered by local identifier, and the order its rows are
/// read in (clause 5.5.9).
///
/// The embedded Context joins the stored rows as one more row of the same
/// projection, so the shared identifier window pages both at once: an offset
/// counts it, a cursor steps past it, and no page leaves it out.
///
/// # Errors
///
/// Returns [`TryFromIntError`] when the embedded entry's hit count exceeds the
/// `bigint` column the projection reads it as.
pub fn listing_page_query(
    detail_level: ContextDetailLevel,
    kind: Option<JsonldContextKindCode>,
    embedded: Option<&ContextMetadata>,
    window: &PageWindow,
) -> Result<(SelectStatement, ReadOrder), TryFromIntError> {
    let mut listed = stored_projection(detail_level);
    if let Some(kind) = kind {
        listed.and_where(kind_filter(kind));
    }
    if let Some(embedded) = embedded {
        listed.union(UnionType::All, embedded_projection(detail_level, embedded)?);
    }
    let mut page = Query::select();
    page.column(Asterisk).from_subquery(listed, Alias::new(LISTED));
    let read_order = apply_identifier_window(&mut page, Expr::col((Alias::new(LISTED), Alias::new(LOCAL_ID))), window);
    Ok((page, read_order))
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
    use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use url::Url;

    /// The listing's SQL and read order for `window`.
    fn listing(
        detail_level: ContextDetailLevel,
        kind: Option<JsonldContextKindCode>,
        embedded: Option<&ContextMetadata>,
        window: &PageWindow,
    ) -> (String, ReadOrder) {
        let (statement, read_order) = listing_page_query(detail_level, kind, embedded, window).expect("a buildable listing");
        (statement.to_string(PostgresQueryBuilder), read_order)
    }

    /// An entry served under its own URL, as the embedded core Context is.
    fn embedded() -> ContextMetadata {
        let url = Url::parse("https://example.com/core.jsonld").expect("a URL");
        let now = Utc::now();
        ContextMetadata::builder()
            .local_id(url.as_str().parse().expect("an identifier"))
            .url(Some(url))
            .serving_iri(None)
            .kind(ContextKind::ImplicitlyCreated)
            .created_at(now)
            .last_usage(now)
            .number_of_hits(0)
            .build()
    }

    #[test]
    fn the_metadata_projection_aliases_id_and_reads_the_kind_code() {
        let (sql, _) = listing(ContextDetailLevel::Full, None, None, &PageWindow::offset(50, 0));
        assert!(sql.contains("\"id\" AS \"local_id\""), "{sql}");
        assert!(sql.contains("\"kind\","), "{sql}");
        assert!(!sql.contains("::text"), "{sql}");
    }

    /// Clause 5.5.9.2: an offset page is cut from the local-identifier order.
    #[test]
    fn an_offset_page_orders_by_local_id_with_limit_and_offset() {
        let (sql, read_order) = listing(ContextDetailLevel::Summary, None, None, &PageWindow::offset(10, 30));
        assert!(sql.contains("ORDER BY \"listed\".\"local_id\" ASC LIMIT 10 OFFSET 30"), "{sql}");
        assert_eq!(read_order, ReadOrder::Ascending);
    }

    #[test]
    fn a_kind_filter_compares_the_kind_code() {
        let (sql, _) = listing(
            ContextDetailLevel::Summary,
            Some(JsonldContextKindCode::Hosted),
            None,
            &PageWindow::offset(10, 0),
        );
        assert!(sql.contains("\"kind\" = 2"), "{sql}");
    }

    /// Clause 5.5.9.1: a forward cursor continues past its boundary.
    #[test]
    fn a_forward_cursor_pages_past_its_boundary() {
        let (sql, read_order) = listing(
            ContextDetailLevel::Summary,
            None,
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
        let (sql, read_order) = listing(ContextDetailLevel::Summary, None, None, &window);
        assert!(sql.contains("\"listed\".\"local_id\" < 'urn:m'"), "{sql}");
        assert!(sql.contains("ORDER BY \"listed\".\"local_id\" DESC LIMIT 10"), "{sql}");
        assert_eq!(read_order, ReadOrder::Descending);
    }

    /// Clause 5.13.3.4 lists the embedded entry with the stored ones: it is
    /// one more row of the windowed collection, never appended after a page.
    #[test]
    fn the_embedded_entry_is_windowed_with_the_stored_rows() {
        let entry = embedded();
        for detail_level in [ContextDetailLevel::Summary, ContextDetailLevel::Full] {
            let (sql, _) = listing(detail_level, None, Some(&entry), &PageWindow::offset(10, 0));
            let union = sql.find("UNION ALL").expect("the embedded row joins the stored rows");
            let window = sql.find("ORDER BY").expect("the page is ordered");
            assert!(union < window, "the window applies to both: {sql}");
            assert!(sql.contains("'https://example.com/core.jsonld' AS \"local_id\""), "{sql}");
        }
    }

    /// A kind filter narrows the stored rows only: whether the embedded entry
    /// is admitted is the caller's decision.
    #[test]
    fn a_kind_filter_leaves_the_embedded_row_alone() {
        let entry = embedded();
        let (sql, _) = listing(
            ContextDetailLevel::Full,
            Some(JsonldContextKindCode::ImplicitlyCreated),
            Some(&entry),
            &PageWindow::offset(10, 0),
        );
        let filter = sql.find("\"kind\" = 3").expect("the stored rows are filtered");
        let union = sql.find("UNION ALL").expect("the embedded row joins");
        assert!(filter < union, "{sql}");
    }
}
