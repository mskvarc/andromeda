//! What the database pool reports to `/health`.
//!
//! The probe lives beside the pool rather than beside the word "healthy":
//! `andromeda-health` declares the port and knows nothing about PostgreSQL, and
//! this is the only place that can answer for it.

use andromeda_health::{
    component::{ComponentDetail, ComponentName},
    details::ComponentDetails,
    indicator::HealthIndicator,
    postgresql_details::PostgreSqlDetails,
    status::HealthStatus,
};
use andromeda_postgresql_seaquery::{literal::int_literal, statement::Statement};
use async_trait::async_trait;
use sea_query::Query;
use sqlx::PgPool;
use std::time::Instant;

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
    #[must_use]
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
        // A trivial round trip that proves the pool can reach the database; the
        // selected constant is never read, only the success of the query.
        let result = Query::select().expr(int_literal(1)).to_query().execute(&self.pool).await;
        // A latency past `u64::MAX` milliseconds is not representable and not
        // reachable; reporting the cap keeps the detail a number either way.
        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        match result {
            Ok(_answered) => ComponentDetail {
                status: HealthStatus::Up,
                details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Reachable {
                    idle_connections: self.pool.num_idle(),
                    ping_latency_ms: elapsed_ms,
                    pool_size: self.pool.size(),
                })),
            },
            Err(error) => ComponentDetail {
                status: HealthStatus::Down,
                details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Unreachable { error: error.to_string() })),
            },
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
