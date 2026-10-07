//! SeaQuery statement builders for the `jsonld_contexts` relation.
//!
//! Every read and write of the JSON-LD context store is assembled here, so the
//! two ports built on the table — the registry and the resolver cache — name its
//! columns and its kind codes in one place.

use crate::jsonld_context::kind_code::stored_kind;
use andromeda_context_registry::detail_level::ContextDetailLevel;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use andromeda_ngsi_ld_query::{page_window::PageWindow, read_order::ReadOrder};
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_postgresql_query::page_window::apply_identifier_window;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{cast::cast, interval::make_interval_secs, pg_type::PostgresType};
use sea_query::{
    Alias,
    Asterisk,
    Condition,
    DeleteStatement,
    Expr,
    ExprTrait,
    InsertStatement,
    Keyword,
    OnConflict,
    Query,
    SelectStatement,
    SimpleExpr,
    UnionType,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use std::num::TryFromIntError;

/// A stored `@context` document bound as the `jsonb` its column holds, from
/// the text it was admitted as.
fn jsonb(payload: &JsonLiteral) -> SimpleExpr {
    cast(Expr::val(payload.text()), PostgresType::Jsonb)
}

/// Match a row by either of its identities: local id or source URL.
///
/// The `id` and `url` columns are the two identities a caller may hold, so a
/// lookup by either resolves the same row.
fn identity_matches(identifier: &str) -> Condition {
    Condition::any()
        .add(Expr::col(JsonldContexts::Id).eq(identifier))
        .add(Expr::col(JsonldContexts::Url).eq(identifier))
}

/// The `@context` payload of the Context identified by `identifier`.
#[must_use]
pub fn payload_by_identifier_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Payload)
        .from(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// A row whose expiry has not passed, measured on the database clock.
///
/// A Context with no expiry never lapses: only the rows the resolver cached
/// carry one, since only they have an origin server that can invalidate them.
fn unexpired() -> Condition {
    Condition::any()
        .add(Expr::col(JsonldContexts::ExpiresAt).is_null())
        .add(Expr::col(JsonldContexts::ExpiresAt).gt(Expr::current_timestamp()))
}

/// The `Cached` kind code.
fn cached_kind() -> SimpleExpr {
    Expr::val(JsonldContextKindCode::Cached.code())
}

/// The `@context` payload of the Context identified by `identifier`, unless it
/// has expired, with what is left of its lifetime.
///
/// This is the resolver's read. Clause 5.13.1 requires the entries flagged
/// `"Cached"` to be invalidated periodically and clause 6.3.16 ties that
/// invalidation to the `Expires` field and the `max-age`/`s-maxage` directives
/// of the response that supplied the document, so a lapsed row stops being a
/// cache hit at the instant it lapses rather than when the sweep that reclaims
/// it next runs.
///
/// The expiry and the database clock it was judged against are projected
/// alongside the payload, because the same clause binds every tier that holds
/// the document: the caller's in-process copy has to lapse when this row does,
/// and it can only do that if the read says how much of the granted span is
/// left. Both instants come from this one statement, so the residual span is
/// measured on a single clock rather than across two.
///
/// The kind and the URL a download was finally read from are projected too: a
/// downloaded document's relative references resolve against that URL, not
/// against the one the row is keyed by.
#[must_use]
pub fn fresh_cached_context_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Payload)
        .column(JsonldContexts::ExpiresAt)
        .column(JsonldContexts::Url)
        .column(JsonldContexts::Kind)
        .column(JsonldContexts::DocumentUrl)
        .expr_as(Expr::current_timestamp(), Alias::new("read_at"))
        .from(JsonldContexts::Table)
        .cond_where(Condition::all().add(identity_matches(identifier)).add(unexpired()))
        .to_owned()
}

/// Store a remotely fetched Context for `lifetime_seconds`, refreshing the entry
/// already held under the same source URL.
///
/// Clause 5.13.1 stores the downloaded content, its URL and the current UTC time
/// alongside the locally unique identifier; a re-download after the previous
/// copy lapsed replaces all three, so the `createdAt` a client inspects reports
/// the age of the content actually held rather than of a copy long since
/// discarded. The URL the content was finally read from, `document_url`, is
/// replaced with it, since the source may redirect elsewhere than it did before.
/// The local identifier is left as it was, since clients hold it, and
/// `number_of_hits` is left to the clause 5.13.3.5 flush.
///
/// The refresh is confined to rows of kind `Cached`: a Hosted Context served
/// from a URL of the broker's own is not the resolver's to overwrite.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the bound value list stops matching the
/// nine-wide column list, which the fixed shape here makes unreachable.
pub fn cached_upsert(local_id: &str, url: &str, document_url: &str, payload: &JsonLiteral, lifetime_seconds: f64) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(JsonldContexts::Table)
        .columns([
            JsonldContexts::Id,
            JsonldContexts::Url,
            JsonldContexts::DocumentUrl,
            JsonldContexts::Kind,
            JsonldContexts::Payload,
            JsonldContexts::CreatedAt,
            JsonldContexts::LastUsage,
            JsonldContexts::NumberOfHits,
            JsonldContexts::ExpiresAt,
        ])
        .values([
            local_id.into(),
            url.into(),
            document_url.into(),
            cached_kind(),
            jsonb(payload),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lifetime_seconds))),
        ])?
        .on_conflict(
            OnConflict::column(JsonldContexts::Url)
                .update_columns([
                    JsonldContexts::DocumentUrl,
                    JsonldContexts::Payload,
                    JsonldContexts::CreatedAt,
                    JsonldContexts::LastUsage,
                    JsonldContexts::ExpiresAt,
                ])
                // Qualified with the table: inside `ON CONFLICT ... DO UPDATE` both the
                // target row and the `excluded` pseudo-table carry `kind`, so a bare
                // reference is ambiguous and PostgreSQL refuses the statement. The guard
                // is on the row already stored, so a `Hosted` context is never
                // overwritten by a cache write.
                .action_and_where(Expr::col((JsonldContexts::Table, JsonldContexts::Kind)).eq(cached_kind()))
                .to_owned(),
        );
    Ok(statement.clone())
}

/// Delete every `Cached` Context whose expiry has passed.
///
/// Clause 5.13.1 requires implementations to periodically invalidate the
/// `"Cached"` `@contexts`, and clause 6.3.16 says that invalidation follows the
/// `Expires` field and the `max-age`/`s-maxage` directives the download carried.
/// Rows of the other two kinds are the client's to delete, never the sweep's.
#[must_use]
pub fn delete_expired_cached() -> DeleteStatement {
    Query::delete()
        .from_table(JsonldContexts::Table)
        .cond_where(
            Condition::all()
                .add(Expr::col(JsonldContexts::Kind).eq(cached_kind()))
                .add(Expr::col(JsonldContexts::ExpiresAt).lte(Expr::current_timestamp())),
        )
        .to_owned()
}

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
                        .map_or_else(|| cast(Keyword::Null.into(), PostgresType::Jsonb), jsonb),
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

/// The id, kind and url of the Context identified by `identifier`.
#[must_use]
pub fn stored_context_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Id)
        .column(JsonldContexts::Kind)
        .column(JsonldContexts::Url)
        .from(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Record one access: refresh the last-usage instant and increment the hit
/// count of the Context identified by `identifier`.
#[must_use]
pub fn record_access_update(identifier: &str) -> UpdateStatement {
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .value(JsonldContexts::NumberOfHits, Expr::col(JsonldContexts::NumberOfHits).add(1))
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Delete the Context identified by `identifier`.
#[must_use]
pub fn delete_by_identifier(identifier: &str) -> DeleteStatement {
    Query::delete()
        .from_table(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Replace a `Cached` Context's document with a new download of it, re-dating
/// the stored copy (clause 5.13.5.4).
///
/// Matched by local id and confined to the `Cached` kind: a reload replaces a
/// download, and only a download has a URL it was finally read from. That URL is
/// replaced with the document, since the source may redirect elsewhere than it
/// did at the first download.
///
/// Clause 5.13.1 tags an entry with the instant its content was stored, so a
/// re-download re-dates it. The expiry moves with it by the span the origin
/// server last granted — clause 6.3.16 fixed that span when the document was
/// first downloaded, and a reload (clause 5.13.5) fetches the same resource from
/// the same server without observing a new one.
#[must_use]
pub fn replace_download_update(local_id: &str, document_url: &str, payload: &JsonLiteral) -> UpdateStatement {
    let granted_lifetime = Expr::col(JsonldContexts::ExpiresAt).sub(Expr::col(JsonldContexts::CreatedAt));
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::Payload, jsonb(payload))
        .value(JsonldContexts::DocumentUrl, document_url)
        .value(JsonldContexts::CreatedAt, Expr::current_timestamp())
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .value(JsonldContexts::ExpiresAt, Expr::current_timestamp().add(granted_lifetime))
        .and_where(Expr::col(JsonldContexts::Id).eq(local_id))
        .and_where(Expr::col(JsonldContexts::Kind).eq(cached_kind()))
        .to_owned()
}

/// Insert a hosted or implicitly-created Context under an exact kind, unless a
/// row already holds `local_id`.
///
/// Registry policy chooses the identifier, and an `ImplicitlyCreated` one is
/// derived from the content, so the same document arrives under the same
/// identifier again. The insert therefore does nothing on an identifier
/// conflict, and the affected-row count tells the caller whether the
/// identifier was free; deciding what an occupied identifier means is left to
/// that caller, which compares the entry holding it.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the bound value list stops matching the
/// seven-wide column list, which the fixed shapes here make unreachable.
pub fn hosted_insert(local_id: &str, serving_url: Option<&str>, kind: JsonldContextKindCode, payload: &JsonLiteral) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(JsonldContexts::Table)
        .columns([
            JsonldContexts::Id,
            JsonldContexts::Url,
            JsonldContexts::Kind,
            JsonldContexts::Payload,
            JsonldContexts::CreatedAt,
            JsonldContexts::LastUsage,
            JsonldContexts::NumberOfHits,
        ])
        .values([
            local_id.into(),
            Expr::val(serving_url),
            kind.code().into(),
            jsonb(payload),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
        ])?
        .on_conflict(OnConflict::column(JsonldContexts::Id).do_nothing().to_owned());
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::statement::{
        cached_upsert,
        delete_expired_cached,
        fresh_cached_context_query,
        hosted_insert,
        listing_page_query,
        payload_by_identifier_query,
        replace_download_update,
        stored_context_query,
    };
    use andromeda_context_registry::detail_level::ContextDetailLevel;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};
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
    fn a_payload_lookup_matches_either_identity() {
        let sql = payload_by_identifier_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"payload\" FROM \"jsonld_contexts\""), "{sql}");
        assert!(sql.contains("\"id\" =") && sql.contains("\"url\" ="), "{sql}");
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

    #[test]
    fn the_stored_context_projection_carries_the_kind_code() {
        let sql = stored_context_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"id\", \"kind\", \"url\""), "{sql}");
    }

    /// Clause 5.13.1: a `"Cached"` entry that has lapsed may not be served, so
    /// the resolver's read passes over it rather than waiting for the sweep.
    #[test]
    fn the_resolver_read_passes_over_a_lapsed_row() {
        let sql = fresh_cached_context_query("https://example.com/c.jsonld").to_string(PostgresQueryBuilder);

        assert!(sql.contains("\"expires_at\" IS NULL"), "a Context with no expiry never lapses: {sql}");
        assert!(sql.contains("\"expires_at\" > CURRENT_TIMESTAMP"), "{sql}");
    }

    /// Clause 6.3.16 binds every tier holding the document to the span the
    /// origin server granted, so the resolver's read reports when the row
    /// lapses and the clock that was judged against, not the payload alone.
    #[test]
    fn the_resolver_read_reports_what_is_left_of_the_granted_lifetime() {
        let sql = fresh_cached_context_query("https://example.com/c.jsonld").to_string(PostgresQueryBuilder);

        assert!(sql.contains("\"payload\""), "{sql}");
        assert!(
            sql.contains("CURRENT_TIMESTAMP AS \"read_at\""),
            "the residual lifetime is judged on one clock: {sql}"
        );
        assert!(sql.contains("\"expires_at\""), "the granted expiry is read alongside the clock: {sql}");
        assert!(sql.contains("\"url\""), "the serving URL is read so clause 5.13.3.5 hits key on it: {sql}");
    }

    /// Clause 6.3.16: the entry's expiry is the lifetime the response granted,
    /// counted from the write rather than baked into the statement text.
    #[test]
    fn a_cached_write_dates_the_row_by_the_granted_lifetime() -> Result<(), SeaQueryError> {
        let (sql, values) = cached_upsert(
            "urn:ngsi-ld:JsonLdContext:1",
            "https://example.com/c.jsonld",
            "https://example.com/c.jsonld",
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"),
            60.0,
        )?
        .build(PostgresQueryBuilder);

        assert!(sql.contains("CURRENT_TIMESTAMP + (make_interval(secs =>"), "{sql}");
        assert!(
            values.0.iter().any(|value| format!("{value:?}").contains("60.0")),
            "the lifetime is bound, not interpolated: {sql}"
        );
        Ok(())
    }

    /// Clause 5.13.1: a re-download replaces the stored copy and its timestamps
    /// while leaving the local identifier clients hold, and the hit count the
    /// clause 5.13.3.5 flush owns, untouched.
    #[test]
    fn a_cached_write_refreshes_an_entry_already_held_under_the_same_url() -> Result<(), SeaQueryError> {
        let sql = cached_upsert(
            "urn:ngsi-ld:JsonLdContext:1",
            "https://example.com/c.jsonld",
            "https://example.com/c.jsonld",
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"),
            60.0,
        )?
        .to_string(PostgresQueryBuilder);

        assert!(sql.contains("ON CONFLICT (\"url\") DO UPDATE"), "{sql}");
        for refreshed in ["\"document_url\"", "\"payload\"", "\"created_at\"", "\"last_usage\"", "\"expires_at\""] {
            assert!(sql.contains(&format!("{refreshed} = ")), "{refreshed} must be refreshed: {sql}");
        }
        assert!(
            !sql.contains("\"number_of_hits\" = \"excluded\""),
            "hit accounting is not the resolver's to reset: {sql}"
        );
        assert!(!sql.contains("\"id\" = \"excluded\""), "the local identifier clients hold is stable: {sql}");
        assert!(sql.contains("\"kind\" = "), "only a Cached row is the resolver's to refresh: {sql}");
        Ok(())
    }

    /// Clause 5.13.5 reload re-dates the stored copy, and clause 6.3.16 keeps
    /// the span the origin server granted: the refreshed entry expires the same
    /// distance out as the copy it replaced, rather than inheriting an instant
    /// that has already passed.
    #[test]
    fn a_reload_carries_the_granted_lifetime_forward() {
        let sql = replace_download_update(
            "urn:ngsi-ld:JsonLdContext:1",
            "https://example.com/c.jsonld",
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"),
        )
        .to_string(PostgresQueryBuilder);

        assert!(sql.contains("\"created_at\" = CURRENT_TIMESTAMP"), "{sql}");
        assert!(
            sql.contains("\"expires_at\" = CURRENT_TIMESTAMP + (\"expires_at\" - \"created_at\")"),
            "the previous span must be parenthesised, or the timestamps would be added: {sql}"
        );
    }

    /// A content-derived identifier arrives again with the same document, so
    /// the insert of a hosted or implicitly created Context does nothing on an
    /// identifier conflict and leaves the entry already there untouched.
    #[test]
    fn a_hosted_insert_does_nothing_on_an_identifier_conflict() -> Result<(), SeaQueryError> {
        let sql = hosted_insert(
            "urn:ngsi-ld:JsonLdContext:1",
            None,
            JsonldContextKindCode::ImplicitlyCreated,
            &JsonLiteral::admit(r#"{"@context": []}"#).expect("admissible"),
        )?
        .to_string(PostgresQueryBuilder);

        assert!(sql.contains("ON CONFLICT (\"id\") DO NOTHING"), "{sql}");
        Ok(())
    }

    /// A download is stored with the URL it was finally read from, bound as a
    /// value beside the URL the row is keyed by, so a redirected document's
    /// relative references resolve against the same base when the row is read
    /// back.
    #[test]
    fn a_cached_write_stores_the_url_the_document_was_finally_read_from() -> Result<(), SeaQueryError> {
        let (sql, values) = cached_upsert(
            "urn:ngsi-ld:JsonLdContext:1",
            "https://example.com/c.jsonld",
            "https://mirror.example.com/v2/c.jsonld",
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"),
            60.0,
        )?
        .build(PostgresQueryBuilder);

        assert!(sql.contains("\"url\", \"document_url\", \"kind\""), "{sql}");
        assert!(
            values
                .0
                .iter()
                .any(|value| format!("{value:?}").contains("https://mirror.example.com/v2/c.jsonld")),
            "the document URL is bound: {sql}"
        );
        Ok(())
    }

    /// The resolver's read reports the kind and the URL a download was finally
    /// read from, so the port can tell a download from a document the broker
    /// stores itself and hand back the base its references resolve against.
    #[test]
    fn the_resolver_read_reports_where_a_download_was_read_from() {
        let sql = fresh_cached_context_query("https://example.com/c.jsonld").to_string(PostgresQueryBuilder);

        assert!(sql.contains("\"kind\""), "{sql}");
        assert!(sql.contains("\"document_url\""), "{sql}");
    }

    /// Clause 5.13.5.4: a reload replaces the download and where it was read
    /// from, and only for a `"Cached"` entry.
    #[test]
    fn a_reload_replaces_the_document_url_of_a_cached_entry_only() {
        let (sql, values) = replace_download_update(
            "urn:ngsi-ld:JsonLdContext:1",
            "https://mirror.example.com/v3/c.jsonld",
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"),
        )
        .build(PostgresQueryBuilder);

        assert!(sql.contains("\"document_url\" = "), "{sql}");
        assert!(sql.contains("\"kind\" = "), "a reload is confined to a Cached row: {sql}");
        assert!(
            values
                .0
                .iter()
                .any(|value| format!("{value:?}").contains("https://mirror.example.com/v3/c.jsonld")),
            "the document URL is bound: {sql}"
        );
    }

    /// Clause 5.13.1 and clause 6.3.16: the sweep reclaims lapsed `"Cached"`
    /// rows and nothing else.
    #[test]
    fn the_sweep_reclaims_only_lapsed_cached_rows() {
        let sql = delete_expired_cached().to_string(PostgresQueryBuilder);

        assert!(sql.starts_with("DELETE FROM \"jsonld_contexts\""), "{sql}");
        assert!(sql.contains("\"kind\" = 1"), "{sql}");
        assert!(sql.contains("\"expires_at\" <= CURRENT_TIMESTAMP"), "{sql}");
    }
}
