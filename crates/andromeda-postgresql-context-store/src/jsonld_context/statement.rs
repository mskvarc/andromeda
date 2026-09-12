//! SeaQuery statement builders for the `jsonld_contexts` relation.
//!
//! Every read and write of the JSON-LD context store is assembled here, so the
//! two ports built on the table — the registry and the resolver cache — name its
//! columns and spell its enum cast in one place.

use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{cast::cast, interval::make_interval_secs, pg_type::PostgresType};
use sea_query::{
    Alias,
    Condition,
    DeleteStatement,
    Expr,
    ExprTrait,
    InsertStatement,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use serde_json::Value;

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

/// The `Cached` kind label, cast to the enum the column carries.
fn cached_kind() -> SimpleExpr {
    cast(Expr::val(<&str>::from(&ContextKind::Cached)), PostgresType::JsonldContextKind)
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
#[must_use]
pub fn fresh_cached_context_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Payload)
        .column(JsonldContexts::ExpiresAt)
        .column(JsonldContexts::Url)
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
/// discarded. The local identifier is left as it was, since clients hold it, and
/// `number_of_hits` is left to the clause 5.13.3.5 flush.
///
/// The refresh is confined to rows of kind `Cached`: a Hosted Context served
/// from a URL of the broker's own is not the resolver's to overwrite.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the bound value list stops matching the
/// eight-wide column list, which the fixed shape here makes unreachable.
pub fn cached_upsert(local_id: &str, url: &str, payload: Value, lifetime_seconds: f64) -> Result<InsertStatement, SeaQueryError> {
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
            JsonldContexts::ExpiresAt,
        ])
        .values([
            local_id.into(),
            url.into(),
            // A bound text parameter does not implicitly coerce to the enum
            // column the way an unknown-typed literal would, so the label is cast.
            cached_kind(),
            payload.into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lifetime_seconds))),
        ])?
        .on_conflict(
            OnConflict::column(JsonldContexts::Url)
                .update_columns([
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
/// `id` answers under `local_id` and the enum `kind` is projected as text, so
/// the row decodes the same whether it came from the list or the point lookup.
fn metadata_columns(statement: &mut SelectStatement) {
    statement
        .expr_as(Expr::col(JsonldContexts::Id), Alias::new("local_id"))
        .column(JsonldContexts::Url)
        .expr_as(cast(Expr::col(JsonldContexts::Kind), PostgresType::Text), Alias::new("kind"))
        .column(JsonldContexts::CreatedAt)
        .column(JsonldContexts::ExpiresAt)
        .column(JsonldContexts::LastUsage)
        .column(JsonldContexts::NumberOfHits)
        .column(JsonldContexts::ExtraInfo)
        .from(JsonldContexts::Table);
}

/// A `kind = $k::jsonld_context_kind` filter, cast from the bound label.
fn kind_filter(kind: &str) -> SimpleExpr {
    Expr::col(JsonldContexts::Kind).eq(cast(Expr::val(kind), PostgresType::JsonldContextKind))
}

/// A page of stored Contexts, ordered by id, optionally of one kind and after a
/// cursor.
///
/// The kind and cursor are absent-or-present filters applied in Rust rather than
/// as `$n IS NULL OR …` guards, so a page carries only the predicates it needs.
fn paged_where(statement: &mut SelectStatement, kind: Option<&str>, after: Option<&str>, limit: i64) {
    if let Some(kind) = kind {
        statement.and_where(kind_filter(kind));
    }
    if let Some(after) = after {
        statement.and_where(Expr::col(JsonldContexts::Id).gt(after));
    }
    // A negative page size is an absent bound, not a narrower one, so it
    // clamps to zero rather than wrapping into an enormous unsigned width.
    statement.order_by(JsonldContexts::Id, Order::Asc).limit(u64::try_from(limit).unwrap_or(0));
}

/// The full-metadata page of stored Contexts.
#[must_use]
pub fn metadata_page_query(kind: Option<&str>, after: Option<&str>, limit: i64) -> SelectStatement {
    let mut statement = Query::select();
    metadata_columns(&mut statement);
    paged_where(&mut statement, kind, after, limit);
    statement.clone()
}

/// The summary (id and url) page of stored Contexts.
#[must_use]
pub fn summary_page_query(kind: Option<&str>, after: Option<&str>, limit: i64) -> SelectStatement {
    let mut statement = Query::select();
    statement.column(JsonldContexts::Id).column(JsonldContexts::Url).from(JsonldContexts::Table);
    paged_where(&mut statement, kind, after, limit);
    statement.clone()
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
        .expr_as(cast(Expr::col(JsonldContexts::Kind), PostgresType::Text), Alias::new("kind"))
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

/// Replace a Context's payload, re-dating the stored copy.
///
/// Matched by local id alone: a replacement addresses the Context this broker
/// hosts, not one it merely cached under a source URL.
///
/// Clause 5.13.1 tags an entry with the instant its content was stored, so a
/// re-download re-dates it. The expiry moves with it by the span the origin
/// server last granted — clause 6.3.16 fixed that span when the document was
/// first downloaded, and a reload (clause 5.13.5) fetches the same resource from
/// the same server without observing a new one. An entry with no expiry keeps
/// none, since `NULL` propagates through the arithmetic.
#[must_use]
pub fn replace_payload_update(local_id: &str, payload: Value) -> UpdateStatement {
    let granted_lifetime = Expr::col(JsonldContexts::ExpiresAt).sub(Expr::col(JsonldContexts::CreatedAt));
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::Payload, payload)
        .value(JsonldContexts::CreatedAt, Expr::current_timestamp())
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .value(JsonldContexts::ExpiresAt, Expr::current_timestamp().add(granted_lifetime))
        .and_where(Expr::col(JsonldContexts::Id).eq(local_id))
        .to_owned()
}

/// Insert a hosted or implicitly-created Context under an exact kind.
///
/// # Errors
///
/// Returns [`SeaQueryError`] when the bound value list stops matching the
/// seven-wide column list, which the fixed shapes here make unreachable.
pub fn hosted_insert(local_id: &str, serving_url: Option<&str>, kind_label: &str, payload: Value) -> Result<InsertStatement, SeaQueryError> {
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
            cast(Expr::val(kind_label), PostgresType::JsonldContextKind),
            payload.into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
        ])?;
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::statement::{
        cached_upsert,
        delete_expired_cached,
        fresh_cached_context_query,
        metadata_page_query,
        payload_by_identifier_query,
        replace_payload_update,
        stored_context_query,
        summary_page_query,
    };
    use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};
    use serde_json::json;

    #[test]
    fn a_payload_lookup_matches_either_identity() {
        let sql = payload_by_identifier_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"payload\" FROM \"jsonld_contexts\""), "{sql}");
        assert!(sql.contains("\"id\" =") && sql.contains("\"url\" ="), "{sql}");
    }

    #[test]
    fn the_metadata_projection_aliases_id_and_casts_kind_to_text() {
        let sql = metadata_page_query(None, None, 50).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"id\" AS \"local_id\""), "{sql}");
        assert!(sql.contains("::text AS \"kind\""), "{sql}");
        assert!(sql.contains("ORDER BY \"id\" ASC"), "{sql}");
        assert!(sql.contains("LIMIT 50"), "{sql}");
    }

    #[test]
    fn a_kind_filter_casts_the_bound_label_to_the_enum() {
        let sql = summary_page_query(Some("Hosted"), None, 10).to_string(PostgresQueryBuilder);
        assert!(sql.contains("::jsonld_context_kind"), "{sql}");
    }

    #[test]
    fn a_cursor_pages_forward_by_id() {
        let sql = summary_page_query(None, Some("urn:a"), 10).to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"id\" >"), "{sql}");
    }

    #[test]
    fn the_stored_context_projection_carries_kind_as_text() {
        let sql = stored_context_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("::text AS \"kind\""), "{sql}");
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
        let (sql, values) = cached_upsert("urn:ngsi-ld:Context:1", "https://example.com/c.jsonld", json!({"@context": {}}), 60.0)?.build(PostgresQueryBuilder);

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
        let sql = cached_upsert("urn:ngsi-ld:Context:1", "https://example.com/c.jsonld", json!({"@context": {}}), 60.0)?.to_string(PostgresQueryBuilder);

        assert!(sql.contains("ON CONFLICT (\"url\") DO UPDATE"), "{sql}");
        for refreshed in ["\"payload\"", "\"created_at\"", "\"last_usage\"", "\"expires_at\""] {
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
        let sql = replace_payload_update("urn:ngsi-ld:Context:1", json!({"@context": {}})).to_string(PostgresQueryBuilder);

        assert!(sql.contains("\"created_at\" = CURRENT_TIMESTAMP"), "{sql}");
        assert!(
            sql.contains("\"expires_at\" = CURRENT_TIMESTAMP + (\"expires_at\" - \"created_at\")"),
            "the previous span must be parenthesised, or the timestamps would be added: {sql}"
        );
    }

    /// Clause 5.13.1 and clause 6.3.16: the sweep reclaims lapsed `"Cached"`
    /// rows and nothing else.
    #[test]
    fn the_sweep_reclaims_only_lapsed_cached_rows() {
        let sql = delete_expired_cached().to_string(PostgresQueryBuilder);

        assert!(sql.starts_with("DELETE FROM \"jsonld_contexts\""), "{sql}");
        assert!(sql.contains("'Cached'::jsonld_context_kind"), "{sql}");
        assert!(sql.contains("\"expires_at\" <= CURRENT_TIMESTAMP"), "{sql}");
    }
}
