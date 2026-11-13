//! The statements over `posted_queries` rows: hold one, read a live one, sweep
//! the expired ones, and the lock every write takes first.
//!
//! A hold, its quota pass and the expiry sweep each run in a transaction that
//! takes the tenant's posted-query lock before touching a row. They run one at
//! a time, so a pass deleting several rows never waits on a row another holds
//! while that one waits on its own.

use crate::{
    posted_query::{
        kind_code::stored_kind,
        mapper::{held_row, stored_digest},
    },
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
use andromeda_posted_query::{
    error::PostedQueryError,
    posted_query::{PostedQuery, PostedQueryKind},
};
use andromeda_postgresql_schema::posted_query::PostedQueries;
use andromeda_postgresql_seaquery::{advisory_lock::advisory_xact_lock, clock::now, statement::Statement};
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, InsertStatement, OnConflict, Query, SelectStatement, SimpleExpr};
use sqlx::{Postgres, Transaction};

/// Take the tenant's posted-query lock for the rest of `transaction`.
pub(crate) async fn lock_writes(transaction: &mut Transaction<'_, Postgres>) -> Result<(), PostedQueryError> {
    Query::select()
        .expr(advisory_xact_lock(Expr::val("andromeda.posted_query")))
        .to_owned()
        .to_query()
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<PostedQueryError>(error))?;
    Ok(())
}

/// `INSERT INTO posted_queries (...) VALUES (...) ON CONFLICT (digest) DO
/// UPDATE SET expires_at = excluded.expires_at`.
///
/// The digest covers every other column, so a row already held under it holds
/// the same query, and only its expiry moves.
///
/// # Errors
///
/// [`PostedQueryError::Unencodable`] when the query cannot be written as
/// JSON, and the persistence failure of a statement that cannot be built.
pub(crate) fn hold_statement(query: &PostedQuery, expires_at: DateTime<Utc>) -> Result<InsertStatement, PostedQueryError> {
    let (columns, values): (Vec<_>, Vec<_>) = held_row(query, expires_at)?.into_iter().unzip();
    let mut statement = Query::insert();
    statement
        .into_table(PostedQueries::Table)
        .columns(columns)
        .values(values)
        .map_err(|error| persistence_error::<PostedQueryError>(error))?
        .on_conflict(OnConflict::column(PostedQueries::Digest).update_column(PostedQueries::ExpiresAt).to_owned());
    Ok(statement)
}

/// The query, link context and body context of the live query `digest` names
/// among those posted to `kind`.
pub(crate) fn live_statement(digest: &QueryDigest, kind: PostedQueryKind) -> SelectStatement {
    Query::select()
        .columns([PostedQueries::Query, PostedQueries::ContextLink, PostedQueries::ContextBody])
        .from(PostedQueries::Table)
        .and_where(Expr::col(PostedQueries::Digest).eq(stored_digest(digest)))
        .and_where(Expr::col(PostedQueries::Kind).eq(stored_kind(kind).code()))
        .and_where(live())
        .to_owned()
}

/// `DELETE FROM posted_queries WHERE expires_at <= <cutoff>`.
pub(crate) fn expired_statement(cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(PostedQueries::Table)
        .and_where(Expr::col(PostedQueries::ExpiresAt).lte(cutoff))
        .to_owned()
}

/// The predicate selecting the queries still live at the database clock.
pub(crate) fn live() -> SimpleExpr {
    Expr::col(PostedQueries::ExpiresAt).gt(now())
}

#[cfg(test)]
mod tests {
    use crate::posted_query::lifecycle::{expired_statement, hold_statement, live_statement};
    use andromeda_jsonld::request_context::RequestContext;
    use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
    use andromeda_posted_query::posted_query::{PostedQuery, PostedQueryKind};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_held_query_moves_only_the_expiry_of_the_row_its_digest_names() {
        let body = serde_json::from_str(r#"{"type":"Query","entities":[{"type":"Vehicle"}]}"#).expect("a clause 5.2.23 Query");
        let query = PostedQuery::new(PostedQueryKind::Entities, body, RequestContext::Default).expect("serialises");
        let (sql, _) = hold_statement(&query, Utc::now()).expect("builds").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"INSERT INTO "posted_queries" ("kind", "expires_at", "digest", "query", "context_link", "context_body") VALUES ($1, $2, $3, $4::json, $5, $6::json) ON CONFLICT ("digest") DO UPDATE SET "expires_at" = "excluded"."expires_at""#
        );
    }

    #[test]
    fn a_query_is_read_only_while_live_and_only_through_the_api_it_was_posted_to() {
        let (sql, _) = live_statement(&QueryDigest::of(None), PostedQueryKind::Temporal).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "query", "context_link", "context_body" FROM "posted_queries" WHERE "digest" = $1 AND "kind" = $2 AND "expires_at" > NOW()"#
        );
    }

    #[test]
    fn the_sweep_deletes_the_queries_due_by_the_cutoff() {
        let sql = expired_statement(DateTime::<Utc>::UNIX_EPOCH).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "posted_queries" WHERE "expires_at" <= '1970-01-01 00:00:00.000000 +00:00'"#);
    }
}
