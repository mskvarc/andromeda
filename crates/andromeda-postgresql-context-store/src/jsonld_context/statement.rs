//! SeaQuery statement builders for the `jsonld_contexts` relation.
//!
//! Every read and write of the JSON-LD context store is assembled here, apart
//! from the clause 5.13.3.4 listing, so the two ports built on the table (the
//! registry and the resolver cache) name its columns and its kind codes in one
//! place.

use andromeda_json_literal::JsonLiteral;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
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
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
    error::Error as SeaQueryError,
};

/// A stored `@context` document bound as the `json` its column holds: the
/// text it was admitted as, cast in the statement so the server keeps it
/// verbatim instead of parsing it into a tree.
pub(crate) fn json(payload: &JsonLiteral) -> SimpleExpr {
    cast(Expr::val(payload.text()), PostgresType::Json)
}

/// Match a row by either of its identities: local id or source URL.
///
/// The `id` and `url` columns are the two identities a caller may hold, so a
/// lookup by either resolves the same row.
pub(crate) fn identity_matches(identifier: &str) -> Condition {
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
            json(payload),
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
/// server last granted: clause 6.3.16 fixed that span when the document was
/// first downloaded, and a reload (clause 5.13.5) fetches the same resource from
/// the same server without observing a new one.
#[must_use]
pub fn replace_download_update(local_id: &str, document_url: &str, payload: &JsonLiteral) -> UpdateStatement {
    let granted_lifetime = Expr::col(JsonldContexts::ExpiresAt).sub(Expr::col(JsonldContexts::CreatedAt));
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::Payload, json(payload))
        .value(JsonldContexts::DocumentUrl, document_url)
        .value(JsonldContexts::CreatedAt, Expr::current_timestamp())
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .value(JsonldContexts::ExpiresAt, Expr::current_timestamp().add(granted_lifetime))
        .and_where(Expr::col(JsonldContexts::Id).eq(local_id))
        .and_where(Expr::col(JsonldContexts::Kind).eq(cached_kind()))
        .to_owned()
}

/// Insert a hosted or implicitly-created Context under an exact kind, unless a
/// row already holds `local_id` or `serving_url`.
///
/// Registry policy chooses the identifier, and an `ImplicitlyCreated` one is
/// derived from the content, so the same document arrives under the same
/// identifier and serving URL again. The insert therefore does nothing on a
/// conflict, and the affected-row count tells the caller whether the slot was
/// free; deciding what an occupied slot means is left to that caller, which
/// compares the entry holding it.
///
/// The conflict names no target, so every unique index arbitrates it. With
/// `id` alone as the target, two concurrent inserts of one document collide on
/// the `url` index outside the speculative insertion, and the loser fails with
/// a unique violation instead of doing nothing.
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
            json(payload),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
        ])?
        .on_conflict(OnConflict::new().do_nothing().to_owned());
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::statement::{
        cached_upsert,
        delete_expired_cached,
        fresh_cached_context_query,
        hosted_insert,
        payload_by_identifier_query,
        replace_download_update,
        stored_context_query,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};

    #[test]
    fn a_context_document_is_bound_as_text_cast_to_json() -> Result<(), SeaQueryError> {
        let document = JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible");
        let statements = [
            cached_upsert(
                "urn:ngsi-ld:JsonLdContext:1",
                "https://example.com/c.jsonld",
                "https://example.com/c.jsonld",
                &document,
                60.0,
            )?
            .to_string(PostgresQueryBuilder),
            replace_download_update("urn:ngsi-ld:JsonLdContext:1", "https://example.com/c.jsonld", &document).to_string(PostgresQueryBuilder),
            hosted_insert("urn:ngsi-ld:JsonLdContext:1", None, JsonldContextKindCode::Hosted, &document)?.to_string(PostgresQueryBuilder),
        ];
        for sql in statements {
            assert!(sql.contains(r#"'{"@context":{}}'::json"#), "{sql}");
            assert!(!sql.contains("::jsonb"), "{sql}");
        }
        Ok(())
    }

    #[test]
    fn a_payload_lookup_matches_either_identity() {
        let sql = payload_by_identifier_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"payload\" FROM \"jsonld_contexts\""), "{sql}");
        assert!(sql.contains("\"id\" =") && sql.contains("\"url\" ="), "{sql}");
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

    /// A content-derived identifier arrives again with the same document and
    /// the same serving URL, possibly from a concurrent request, so the insert
    /// does nothing on a conflict with any unique index and leaves the entry
    /// already there untouched.
    #[test]
    fn a_hosted_insert_does_nothing_on_a_conflict_with_any_unique_index() -> Result<(), SeaQueryError> {
        let sql = hosted_insert(
            "urn:ngsi-ld:JsonLdContext:1",
            Some("https://broker.example/jsonldContexts/urn:ngsi-ld:JsonLdContext:1"),
            JsonldContextKindCode::ImplicitlyCreated,
            &JsonLiteral::admit(r#"{"@context": []}"#).expect("admissible"),
        )?
        .to_string(PostgresQueryBuilder);

        assert!(sql.ends_with("ON CONFLICT  DO NOTHING"), "no conflict target: {sql}");
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
