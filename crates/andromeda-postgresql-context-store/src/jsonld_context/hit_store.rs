//! Direct PostgreSQL implementation of durable Context hit accounting.

use crate::jsonld_context::repo::JsonLdContextRepository;
use andromeda_context_registry::{
    error::ContextRegistryError,
    hit_store::{ContextHitStore, ContextHitStoreFuture},
};
use andromeda_jsonld::hit_counter::HitDelta;

impl ContextHitStore for JsonLdContextRepository {
    fn flush_hits<'a>(&'a self, deltas: &'a [HitDelta]) -> ContextHitStoreFuture<'a> {
        Box::pin(async move {
            if deltas.is_empty() {
                return Ok(());
            }
            let urls: Vec<&str> = deltas.iter().map(|delta| delta.url.as_str()).collect();
            let counts = deltas
                .iter()
                .map(|delta| {
                    i64::try_from(delta.count).map_err(|_| ContextRegistryError::Invariant {
                        detail: format!("Context hit delta exceeds PostgreSQL bigint: {}", delta.count),
                    })
                })
                .collect::<Result<Vec<i64>, ContextRegistryError>>()?;
            sqlx::query(
                r"
                UPDATE jsonld_contexts
                SET number_of_hits = jsonld_contexts.number_of_hits + delta.count,
                    last_usage = NOW()
                FROM UNNEST($1::text[], $2::bigint[]) AS delta(url, count)
                WHERE jsonld_contexts.url = delta.url
                ",
            )
            .bind(&urls)
            .bind(&counts)
            .execute(&self.pool)
            .await
            .map_err(|error| ContextRegistryError::Persistence { detail: error.to_string() })?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::repo::JsonLdContextRepository;
    use andromeda_context_registry::{error::ContextRegistryError, hit_store::ContextHitStore};
    use sqlx::postgres::PgPool;

    #[tokio::test]
    async fn empty_hit_batch_does_not_open_a_database_connection() -> Result<(), ContextRegistryError> {
        let pool = PgPool::connect_lazy("postgres://postgres:postgres@localhost/andromeda")
            .map_err(|error| ContextRegistryError::Invariant { detail: error.to_string() })?;
        let repository = JsonLdContextRepository::new(&pool);
        repository.flush_hits(&[]).await
    }
}
