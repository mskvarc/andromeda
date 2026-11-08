//! PostgreSQL repository handle shared by the Context Registry and resolver-cache capabilities.

use sqlx::postgres::PgPool;

/// PostgreSQL-backed repository for JSON-LD Context documents.
#[derive(Debug)]
pub struct JsonLdContextRepository {
    /// The tenant database pool shared by all Context persistence capabilities.
    pub(crate) pool: PgPool,
}

impl JsonLdContextRepository {
    /// Create a Context repository backed by the given tenant connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> JsonLdContextRepository {
        JsonLdContextRepository { pool: pool.clone() }
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::repo::JsonLdContextRepository;
    use sqlx::postgres::PgPool;

    #[tokio::test]
    async fn repository_can_hold_a_lazy_pool_without_connecting() -> Result<(), sqlx::Error> {
        let pool = PgPool::connect_lazy("postgres://postgres:postgres@localhost/andromeda")?;
        let repository = JsonLdContextRepository::new(&pool);
        pretty_assertions::assert_eq!(repository.pool.options().get_max_connections(), pool.options().get_max_connections());
        Ok(())
    }
}
