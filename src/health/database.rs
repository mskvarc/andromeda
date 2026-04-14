//! Database health indicator.
//!
//! Checks PostgreSQL connectivity by acquiring a connection and measuring ping
//! latency, and reports pool statistics (size, idle connections).

use crate::{
    domain::health::{ComponentDetail, ComponentName, HealthStatus},
    health::HealthIndicator,
};
use sqlx::PgPool;
use std::{collections::BTreeMap, future::Future, pin::Pin};

/// Health indicator for the PostgreSQL database backend.
///
/// Acquires a connection from the pool, pings the database, and reports pool
/// statistics alongside measured latency.
pub struct DatabaseHealthIndicator {
    /// Connection pool to check.
    pool: PgPool,
}

impl DatabaseHealthIndicator {
    /// Create a new database health indicator for the given pool.
    pub fn new(pool: PgPool) -> DatabaseHealthIndicator {
        DatabaseHealthIndicator { pool }
    }
}

impl HealthIndicator for DatabaseHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::PostgreSql
    }

    fn check(&self) -> Pin<Box<dyn Future<Output = ComponentDetail> + Send + '_>> {
        Box::pin(async {
            let start = std::time::Instant::now();

            let result = sqlx::query("SELECT 1").execute(&self.pool).await;
            let elapsed_ms = start.elapsed().as_millis();

            match result {
                Ok(_) => {
                    let mut details = BTreeMap::new();
                    details.insert("poolSize".to_owned(), serde_json::Value::from(self.pool.size()));
                    details.insert("idleConnections".to_owned(), serde_json::Value::from(self.pool.num_idle()));
                    details.insert("pingLatencyMs".to_owned(), serde_json::Value::from(elapsed_ms as i64));

                    ComponentDetail {
                        status: HealthStatus::Up,
                        details,
                    }
                }
                Err(err) => {
                    let mut details = BTreeMap::new();
                    details.insert("error".to_owned(), serde_json::Value::String(err.to_string()));

                    ComponentDetail {
                        status: HealthStatus::Down,
                        details,
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn database_indicator_name_is_postgresql() {
        // connect_lazy requires a Tokio context but does not actually connect.
        let pool = PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap_or_else(|_| panic!("connect_lazy should not fail"));
        let indicator = DatabaseHealthIndicator::new(pool);
        pretty_assertions::assert_eq!(indicator.name(), ComponentName::PostgreSql);
    }
}
