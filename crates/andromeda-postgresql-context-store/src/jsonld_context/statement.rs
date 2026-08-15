//! SeaQuery statement builders for the `jsonld_contexts` relation.
//!
//! Every read and write of the JSON-LD context store is assembled here, so the
//! two ports built on the table — the registry and the resolver cache — name its
//! columns and spell its enum cast in one place.

use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Alias, Condition, DeleteStatement, Expr, InsertStatement, Order, Query, SelectStatement, SimpleExpr, UpdateStatement};
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
pub fn payload_by_identifier_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Payload)
        .from(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
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
        .expr_as(cast(Expr::col(JsonldContexts::Kind).into(), PostgresType::Text), Alias::new("kind"))
        .column(JsonldContexts::CreatedAt)
        .column(JsonldContexts::ExpiresAt)
        .column(JsonldContexts::LastUsage)
        .column(JsonldContexts::NumberOfHits)
        .column(JsonldContexts::ExtraInfo)
        .from(JsonldContexts::Table);
}

/// A `kind = $k::jsonld_context_kind` filter, cast from the bound label.
fn kind_filter(kind: &str) -> SimpleExpr {
    Expr::col(JsonldContexts::Kind).eq(cast(Expr::val(kind).into(), PostgresType::JsonldContextKind))
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
    statement.order_by(JsonldContexts::Id, Order::Asc).limit(limit.max(0) as u64);
}

/// The full-metadata page of stored Contexts.
pub fn metadata_page_query(kind: Option<&str>, after: Option<&str>, limit: i64) -> SelectStatement {
    let mut statement = Query::select();
    metadata_columns(&mut statement);
    paged_where(&mut statement, kind, after, limit);
    statement.to_owned()
}

/// The summary (id and url) page of stored Contexts.
pub fn summary_page_query(kind: Option<&str>, after: Option<&str>, limit: i64) -> SelectStatement {
    let mut statement = Query::select();
    statement.column(JsonldContexts::Id).column(JsonldContexts::Url).from(JsonldContexts::Table);
    paged_where(&mut statement, kind, after, limit);
    statement.to_owned()
}

/// The full metadata of the Context identified by `identifier`.
pub fn metadata_by_identifier_query(identifier: &str) -> SelectStatement {
    let mut statement = Query::select();
    metadata_columns(&mut statement);
    statement.cond_where(identity_matches(identifier));
    statement.to_owned()
}

/// The id, kind and url of the Context identified by `identifier`.
pub fn stored_context_query(identifier: &str) -> SelectStatement {
    Query::select()
        .column(JsonldContexts::Id)
        .expr_as(cast(Expr::col(JsonldContexts::Kind).into(), PostgresType::Text), Alias::new("kind"))
        .column(JsonldContexts::Url)
        .from(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Record one access: refresh the last-usage instant and increment the hit
/// count of the Context identified by `identifier`.
pub fn record_access_update(identifier: &str) -> UpdateStatement {
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .value(JsonldContexts::NumberOfHits, Expr::col(JsonldContexts::NumberOfHits).add(1))
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Delete the Context identified by `identifier`.
pub fn delete_by_identifier(identifier: &str) -> DeleteStatement {
    Query::delete()
        .from_table(JsonldContexts::Table)
        .cond_where(identity_matches(identifier))
        .to_owned()
}

/// Replace a hosted Context's payload, refreshing its last-usage instant.
///
/// Matched by local id alone: a replacement addresses the Context this broker
/// hosts, not one it merely cached under a source URL.
pub fn replace_payload_update(local_id: &str, payload: Value) -> UpdateStatement {
    Query::update()
        .table(JsonldContexts::Table)
        .value(JsonldContexts::Payload, payload)
        .value(JsonldContexts::LastUsage, Expr::current_timestamp())
        .and_where(Expr::col(JsonldContexts::Id).eq(local_id))
        .to_owned()
}

/// Insert a hosted or implicitly-created Context under an exact kind.
pub fn hosted_insert(local_id: &str, serving_url: Option<&str>, kind_label: &str, payload: Value) -> Result<InsertStatement, sea_query::error::Error> {
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
            Expr::val(serving_url).into(),
            cast(Expr::val(kind_label).into(), PostgresType::JsonldContextKind),
            payload.into(),
            Expr::current_timestamp().into(),
            Expr::current_timestamp().into(),
            0_i64.into(),
        ])?;
    Ok(statement.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::statement::{metadata_page_query, payload_by_identifier_query, stored_context_query, summary_page_query};
    use sea_query::PostgresQueryBuilder;

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
}
