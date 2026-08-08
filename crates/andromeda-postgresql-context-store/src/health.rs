//! What the database pool reports to `/health`.
//!
//! The probe lives beside the pool rather than beside the word "healthy":
//! `andromeda-health` declares the port and knows nothing about PostgreSQL, and
//! this is the only place that can answer for it.

use andromeda_health::{
    component::{ComponentDetail, ComponentName},
    indicator::HealthIndicator,
    status::HealthStatus,
};
use async_trait::async_trait;
use sqlx::PgPool;
use std::{collections::BTreeMap, time::Instant};

/// Health indicator for the PostgreSQL database backend.
///
/// Acquires a connection from the pool, pings the database, and reports pool
/// statistics alongside the measured latency.
pub struct DatabaseHealthIndicator {
    /// Connection pool to check.
    pool: PgPool,
}

impl DatabaseHealthIndicator {
    /// Report on the given pool.
    pub const fn new(pool: PgPool) -> DatabaseHealthIndicator {
        DatabaseHealthIndicator { pool }
    }
}

#[async_trait]
impl HealthIndicator for DatabaseHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::PostgreSql
    }

    async fn check(&self) -> ComponentDetail {
        let start = Instant::now();
        let result = sqlx::query("SELECT 1").execute(&self.pool).await;
        let elapsed_ms = start.elapsed().as_millis();

        let mut details = BTreeMap::new();
        match result {
            Ok(_answered) => {
                details.insert("poolSize".to_owned(), serde_json::Value::from(self.pool.size()));
                details.insert("idleConnections".to_owned(), serde_json::Value::from(self.pool.num_idle()));
                details.insert("pingLatencyMs".to_owned(), serde_json::Value::from(elapsed_ms as i64));
                ComponentDetail {
                    status: HealthStatus::Up,
                    details,
                }
            }
            Err(error) => {
                details.insert("error".to_owned(), serde_json::Value::String(error.to_string()));
                ComponentDetail {
                    status: HealthStatus::Down,
                    details,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::health::DatabaseHealthIndicator;
    use andromeda_health::{component::ComponentName, indicator::HealthIndicator};
    use sqlx::PgPool;

    #[tokio::test]
    async fn the_indicator_reports_under_the_postgresql_component_name() {
        // `connect_lazy` needs a Tokio context but does not connect.
        let pool = PgPool::connect_lazy("postgres://localhost/nonexistent").expect("a lazy pool");
        let indicator = DatabaseHealthIndicator::new(pool);
        pretty_assertions::assert_eq!(indicator.name(), ComponentName::PostgreSql);
    }
}
