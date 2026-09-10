//! Direct PostgreSQL implementation of periodic cached-Context invalidation.
//!
//! One statement, run on a cadence the broker owns: the clause 5.13.1 sweep
//! that reclaims the `"Cached"` rows whose clause 6.3.16 expiration time has
//! passed. Serving is already safe without it — the resolver's read passes over
//! a lapsed row — so this reclaims storage and keeps clause 5.13.3's listing
//! honest about what the broker still holds.

use crate::jsonld_context::{repo::JsonLdContextRepository, statement::delete_expired_cached};
use andromeda_context_registry::{
    cached_context_invalidation::{CachedContextInvalidation, CachedContextInvalidationFuture, InvalidatedContexts},
    error::ContextRegistryError,
};
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;

impl CachedContextInvalidation for JsonLdContextRepository {
    fn invalidate_expired(&self) -> CachedContextInvalidationFuture<'_> {
        Box::pin(async move {
            let (sql, values) = delete_expired_cached().build_sqlx(PostgresQueryBuilder);
            let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(|error| ContextRegistryError::Persistence { detail: error.to_string() })?;
            Ok(InvalidatedContexts::reclaimed(result.rows_affected()))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::repo::JsonLdContextRepository;
    use andromeda_context_registry::cached_context_invalidation::CachedContextInvalidation;

    /// Clause 5.13.1's periodic pass is driven by the broker against the port,
    /// so a tenant's one Context repository has to supply it alongside the
    /// registry and resolver-cache capabilities built on the same table. What
    /// the pass deletes is asserted where the statement is built.
    #[test]
    fn the_context_repository_supplies_the_invalidation_capability() {
        fn requires_invalidation<T: CachedContextInvalidation>() {}
        requires_invalidation::<JsonLdContextRepository>();
    }
}
