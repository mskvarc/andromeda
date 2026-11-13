//! The `posted_queries` columns a posted query is written as, and the posted
//! query a live row reads back as.
//!
//! The query is stored as the clause 5.2.23 JSON it serialises to, and the
//! context in the one place the POST named it: its `Link` header IRI, its body
//! `@context` as written, or neither.

use crate::{
    json_text::JsonText,
    posted_query::kind_code::stored_kind,
    store_fault::{persistence_error, undecodable},
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_jsonld::request_context::RequestContext;
use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
use andromeda_posted_query::{
    error::PostedQueryError,
    posted_query::{PostedQuery, PostedQueryKind},
};
use andromeda_postgresql_schema::posted_query::PostedQueries;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use chrono::{DateTime, Utc};
use iri_rs::{InvalidIri, IriBuf};
use sea_query::{Expr, Iden, SimpleExpr};
use sqlx::{Row, postgres::PgRow};
use std::str::{self, Utf8Error};
use thiserror::Error;

/// A stored posted-query column that does not read back as the value the
/// broker wrote.
///
/// The broker only ever writes values it read and validated, so each variant
/// is a column holding something the broker never wrote.
#[derive(Debug, Error)]
pub enum StoredPostedQueryError {
    /// The stored query is not a clause 5.2.23 Query.
    #[error("a stored posted query is not a query: {0}")]
    Query(#[source] serde_json::Error),
    /// The stored `Link` context is not an IRI.
    #[error("a stored posted query link context is not an IRI: {0}")]
    ContextLink(#[source] InvalidIri<String>),
    /// The stored body `@context` is not UTF-8 text.
    #[error("a stored posted query body context is not UTF-8: {0}")]
    ContextBodyText(#[source] Utf8Error),
    /// The stored body `@context` is not a JSON literal.
    #[error("a stored posted query body context is not JSON: {0}")]
    ContextBody(#[source] JsonLiteralError),
    /// The row names a context both in a link and in a body, which no request
    /// can.
    #[error("a stored posted query names its context both in a link and in a body")]
    TwoContexts,
}

/// Every column of the row `query` is held in until `expires_at`, with the
/// value it holds.
///
/// # Errors
///
/// [`PostedQueryError::Unencodable`] when the query cannot be written as JSON.
pub(crate) fn held_row(query: &PostedQuery, expires_at: DateTime<Utc>) -> Result<[(PostedQueries, SimpleExpr); 6], PostedQueryError> {
    let document = serde_json::to_string(query.query()).map_err(PostedQueryError::Unencodable)?;
    let context = query.context();
    Ok([
        (PostedQueries::Kind, Expr::val(stored_kind(query.kind()).code())),
        (PostedQueries::ExpiresAt, Expr::val(expires_at)),
        (PostedQueries::Digest, stored_digest(query.digest())),
        (PostedQueries::Query, cast(Expr::val(document), PostgresType::Json)),
        (PostedQueries::ContextLink, Expr::val(context.link_context().map(IriBuf::as_str))),
        (
            PostedQueries::ContextBody,
            cast(Expr::val(context.body_context().map(JsonLiteral::text)), PostgresType::Json),
        ),
    ])
}

/// The value the `digest` column holds for `digest`: its own bytes, which a
/// page link spells in base64url.
pub(crate) fn stored_digest(digest: &QueryDigest) -> SimpleExpr {
    Expr::val(digest.as_bytes().to_vec())
}

/// The posted query a live row of `kind` holds, the row carrying the columns
/// [`PostedQueries::Query`], [`PostedQueries::ContextLink`] and
/// [`PostedQueries::ContextBody`].
///
/// # Errors
///
/// The persistence failure of a column that is missing or does not read back
/// as the value the broker wrote.
pub(crate) fn posted_query_of(kind: PostedQueryKind, row: &PgRow) -> Result<PostedQuery, PostedQueryError> {
    let document: JsonText<'_> = row
        .try_get(PostedQueries::Query.unquoted())
        .map_err(|error| persistence_error::<PostedQueryError>(error))?;
    let link: Option<String> = row
        .try_get(PostedQueries::ContextLink.unquoted())
        .map_err(|error| persistence_error::<PostedQueryError>(error))?;
    let body: Option<JsonText<'_>> = row
        .try_get(PostedQueries::ContextBody.unquoted())
        .map_err(|error| persistence_error::<PostedQueryError>(error))?;
    let query = serde_json::from_slice(document.as_bytes())
        .map_err(StoredPostedQueryError::Query)
        .map_err(|error| PostedQueryError::from(undecodable(&error)))?;
    let context = request_context(link, body).map_err(|error| PostedQueryError::from(undecodable(&error)))?;
    PostedQuery::new(kind, query, context)
}

/// The context a request named, read back from the column it was stored in.
fn request_context(link: Option<String>, body: Option<JsonText<'_>>) -> Result<RequestContext, StoredPostedQueryError> {
    match (link, body) {
        (None, None) => Ok(RequestContext::Default),
        (Some(link), None) => IriBuf::new(link).map(RequestContext::Link).map_err(StoredPostedQueryError::ContextLink),
        (None, Some(body)) => {
            let text = str::from_utf8(body.as_bytes()).map_err(StoredPostedQueryError::ContextBodyText)?;
            JsonLiteral::admit(text).map(RequestContext::Body).map_err(StoredPostedQueryError::ContextBody)
        }
        (Some(_), Some(_)) => Err(StoredPostedQueryError::TwoContexts),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        json_text::JsonText,
        posted_query::mapper::{StoredPostedQueryError, held_row, request_context, stored_digest},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::request_context::RequestContext;
    use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
    use andromeda_posted_query::posted_query::{PostedQuery, PostedQueryKind};
    use chrono::Utc;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    const LINK: &str = "https://example.com/a.jsonld";
    const BODY: &str = r#"{"speed":"https://example.com/speed"}"#;

    #[test]
    fn each_context_reads_back_from_the_column_it_is_stored_in() {
        assert_eq!(request_context(None, None).expect("default"), RequestContext::Default);
        assert_eq!(
            request_context(Some(LINK.to_owned()), None).expect("a link"),
            RequestContext::Link(IriBuf::new(LINK.to_owned()).expect("a valid IRI"))
        );
        assert_eq!(
            request_context(None, Some(JsonText::new(BODY.as_bytes()))).expect("a body context"),
            RequestContext::Body(JsonLiteral::admit(BODY).expect("a context"))
        );
    }

    #[test]
    fn a_row_naming_a_context_in_both_places_does_not_read_back() {
        let read = request_context(Some(LINK.to_owned()), Some(JsonText::new(BODY.as_bytes())));
        assert!(matches!(read, Err(StoredPostedQueryError::TwoContexts)), "{read:?}");
    }

    #[test]
    fn a_link_context_that_is_not_an_iri_does_not_read_back() {
        let read = request_context(Some("not an iri".to_owned()), None);
        assert!(matches!(read, Err(StoredPostedQueryError::ContextLink(_))), "{read:?}");
    }

    /// The column holds the digest's bytes, never the base64url token a page
    /// link spells them with.
    #[test]
    fn a_digest_is_stored_as_its_bytes() {
        let digest = QueryDigest::of(Some("type=Vehicle"));
        let sql = Query::select().expr(stored_digest(&digest)).to_string(PostgresQueryBuilder);
        assert_eq!(sql, format!(r"SELECT '\x{}'", hex::encode_upper(digest.as_bytes())));
    }

    /// The query and a body context are bound as text and cast to `json`, so
    /// the column keeps the text the broker wrote.
    #[test]
    fn a_held_query_binds_every_column_and_casts_its_json() {
        let body = serde_json::from_str(r#"{"type":"Query","entities":[{"type":"Vehicle"}]}"#).expect("a clause 5.2.23 Query");
        let held = PostedQuery::new(
            PostedQueryKind::Temporal,
            body,
            RequestContext::Body(JsonLiteral::admit(BODY).expect("a context")),
        )
        .expect("serialises");
        let (sql, values) = Query::select()
            .exprs(held_row(&held, Utc::now()).expect("encodes").map(|(_, value)| value))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT $1, $2, $3, $4::json, $5, $6::json");
        assert_eq!(values.0.len(), 6);
    }
}
