//! Direct PostgreSQL implementation of the JSON-LD resolver's technical cache port.
//!
//! The rows this writes are the `"Cached"` entries of clause 5.13.1, and that
//! clause requires them to be invalidated periodically — clause 6.3.16 saying
//! how, from the `Expires` field and the `max-age`/`s-maxage` directives of the
//! response that carried the document.
//!
//! Invalidation has two halves, and only one of them is here: a lapsed row
//! stops being a cache hit the instant it lapses, because the read passes over
//! it. Reclaiming lapsed rows is the periodic pass of
//! [`cached_context_invalidation`](crate::jsonld_context::cached_context_invalidation),
//! which runs on a cadence of its own rather than riding on a write — a broker
//! that has stopped downloading new `@contexts` still has to invalidate the
//! ones it holds.

use crate::{
    jsonld_context::{
        mapper::{ContextMapperError, row_document_source},
        repo::JsonLdContextRepository,
        rows::FreshContextRow,
        statement::{cached_upsert, fresh_cached_context_query},
    },
    store_fault::{StoreFault, persistence_failure},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{
    context_cache::{CachedContext, ContextCache, ContextCacheError},
    loader::cache_lifetime::{CacheLifetime, ResidualLifetime},
};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;

#[async_trait]
impl ContextCache for JsonLdContextRepository {
    async fn get(&self, url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
        let (sql, values) = fresh_cached_context_query(url.as_str()).build_sqlx(PostgresQueryBuilder);
        let row: Option<FreshContextRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| ContextCacheError::Read(persistence_failure(&error)))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let Some(residual) = residual_lifetime(row.expires_at, row.read_at) else {
            return Ok(None);
        };
        let source = row_document_source(row.kind, row.document_url).map_err(|error| ContextCacheError::Read(persistence_failure(&error)))?;
        // Clause 5.13.3.5 keys `numberOfHits` on the stored URL; a row reached
        // by its local id carries the serving URL here so the access is
        // attributed to the same key the metadata reports.
        let canonical_url = row
            .url
            .map(|url| IriBuf::new(url).map_err(ContextMapperError::ServingIri))
            .transpose()
            .map_err(|error| ContextCacheError::Read(persistence_failure(&error)))?;
        Ok(Some(CachedContext {
            payload: row.payload.0,
            residual,
            canonical_url,
            source,
        }))
    }

    async fn put(&self, url: &IriBuf, document_url: &IriBuf, payload: &JsonLiteral, lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
        let local_id = cached_context_identifier()?;
        let statement =
            cached_upsert(local_id.as_ref(), url.as_str(), document_url.as_str(), payload, lifetime.as_seconds()).map_err(|error| write_failure(error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.pool)
            .await
            .map_err(|error| write_failure(error))?;
        Ok(())
    }
}

/// What is left of a stored row's lifetime, on the clock that read it.
///
/// `None` means nothing is left, which the port reports as a miss rather than a
/// hit: clause 5.13.1 forbids serving a lapsed `"Cached"` entry. The read
/// already excludes lapsed rows, so this only settles the sliver of time
/// between the predicate and the projection — and settles it the safe way.
///
/// A row with no expiry at all is not a cached download but a Context the
/// broker hosts under its own URL, which no origin server can make stale.
fn residual_lifetime(expires_at: Option<DateTime<Utc>>, read_at: DateTime<Utc>) -> Option<ResidualLifetime> {
    let Some(expires_at) = expires_at else {
        return Some(ResidualLifetime::Unbounded);
    };
    let remaining = expires_at.signed_duration_since(read_at).to_std().ok()?;
    CacheLifetime::new(remaining).map(ResidualLifetime::Bounded)
}

/// Collapse a backend failure into the port's write category.
#[track_caller]
fn write_failure(error: impl StoreFault) -> ContextCacheError {
    ContextCacheError::Write(persistence_failure(&error))
}

/// Mint the local identifier used for a newly cached remote Context.
///
/// A random one: the original URL is already a second, stable identity clients
/// may address the entry by (Table 6.30.2-1), so deriving the local identifier
/// from it would add nothing. A refresh of the same URL keeps the identifier
/// the first download minted, since the upsert leaves the `id` column alone.
fn cached_context_identifier() -> Result<JsonLdContextId, ContextCacheError> {
    JsonLdContextId::mint().map_err(|error| write_failure(error))
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::context_cache::{cached_context_identifier, residual_lifetime};
    use andromeda_jsonld::{
        context_cache::ContextCacheError,
        loader::cache_lifetime::{CacheLifetime, ResidualLifetime},
    };
    use chrono::{TimeDelta, Utc};

    #[test]
    fn cached_context_identifier_is_a_json_ld_context_identifier() -> Result<(), ContextCacheError> {
        assert!(cached_context_identifier()?.to_string().starts_with("urn:ngsi-ld:JsonLdContext:"));
        Ok(())
    }

    /// Clause 6.3.16: what the read hands back is the remainder of the span the
    /// origin server granted, so a row written with `max-age=30` that has been
    /// held for twenty seconds has ten seconds left — not thirty, and not the
    /// configured heuristic.
    #[test]
    fn a_row_reports_the_remainder_of_the_span_it_was_granted() {
        let read_at = Utc::now();
        let expires_at = read_at + TimeDelta::try_seconds(10).expect("ten seconds is a valid delta");

        pretty_assertions::assert_eq!(
            residual_lifetime(Some(expires_at), read_at),
            Some(ResidualLifetime::Bounded(
                CacheLifetime::from_seconds(10).expect("ten seconds is a positive lifetime")
            ))
        );
    }

    /// Clause 5.13.1 forbids serving a lapsed `"Cached"` entry, so a row that
    /// has nothing left is a miss and resolution reaches the origin server.
    #[test]
    fn a_row_with_nothing_left_is_not_a_hit() {
        let read_at = Utc::now();
        let lapsed = read_at - TimeDelta::try_seconds(1).expect("one second is a valid delta");

        pretty_assertions::assert_eq!(residual_lifetime(Some(lapsed), read_at), None);
        pretty_assertions::assert_eq!(residual_lifetime(Some(read_at), read_at), None);
    }

    /// A Context the broker hosts records no expiry, and no origin server can
    /// invalidate it.
    #[test]
    fn a_row_without_an_expiry_never_lapses() {
        pretty_assertions::assert_eq!(residual_lifetime(None, Utc::now()), Some(ResidualLifetime::Unbounded));
    }
}
