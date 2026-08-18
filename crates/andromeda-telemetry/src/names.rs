//! The metric and label names the broker emits.
//!
//! Names follow the Prometheus conventions: snake_case, an `andromeda_` prefix,
//! base units in the name (`_seconds`, `_total`). Label values are kebab-case
//! and come from the enums in the sibling modules, so no free-form string
//! ever becomes a series dimension.

/// Histogram of wall-clock seconds spent awaiting one database statement.
pub const DB_STATEMENT_SECONDS: &str = "andromeda_db_statement_seconds";
/// Counter of database statements that returned an error.
pub const DB_STATEMENT_FAILURES_TOTAL: &str = "andromeda_db_statement_failures_total";
/// Gauge of open connections in one tenant's pool.
pub const DB_POOL_CONNECTIONS: &str = "andromeda_db_pool_connections";
/// Gauge of idle connections in one tenant's pool.
pub const DB_POOL_IDLE_CONNECTIONS: &str = "andromeda_db_pool_idle_connections";
/// Counter of durable-queue item outcomes.
pub const QUEUE_OUTCOMES_TOTAL: &str = "andromeda_queue_outcomes_total";
/// Histogram of subscription candidates surviving each funnel stage per event.
pub const SUBSCRIPTION_CANDIDATES: &str = "andromeda_subscription_candidates";

/// The `statement` label naming a database statement.
pub const LABEL_STATEMENT: &str = "statement";
/// The `tenant` label naming the tenant a pool serves.
pub const LABEL_TENANT: &str = "tenant";
/// The `queue` label naming a durable queue.
pub const LABEL_QUEUE: &str = "queue";
/// The `outcome` label naming what happened to a queue item.
pub const LABEL_OUTCOME: &str = "outcome";
/// The `stage` label naming a subscription funnel stage.
pub const LABEL_STAGE: &str = "stage";
