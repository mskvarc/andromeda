//! The metric and label names the broker emits.
//!
//! Names follow the Prometheus conventions: `snake_case`, an `andromeda_` prefix,
//! base units in the name (`_seconds`, `_total`). Label values are kebab-case
//! and come from the enums in the sibling modules, so no free-form string
//! ever becomes a series dimension.

/// Counter of HTTP requests handled, labelled by endpoint, method and status.
///
/// Emitted by the `axum-prometheus` layer in `andromeda-http-stack`, whose
/// `with_prefix("andromeda")` supplies the `andromeda_` half of this literal.
/// The string is duplicated here deliberately so this crate owns the rendered
/// name; there is no shared symbol and therefore no dependency edge.
pub const HTTP_REQUESTS_TOTAL: &str = "andromeda_http_requests_total";
/// Histogram of HTTP request durations in seconds, labelled by endpoint, method and status.
///
/// See [`HTTP_REQUESTS_TOTAL`] for the prefix-coupling note.
pub const HTTP_REQUEST_DURATION_SECONDS: &str = "andromeda_http_requests_duration_seconds";
/// Gauge of in-flight HTTP requests, labelled by endpoint and method.
///
/// See [`HTTP_REQUESTS_TOTAL`] for the prefix-coupling note.
pub const HTTP_REQUESTS_PENDING: &str = "andromeda_http_requests_pending";
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
/// Counter of adaptive PostgreSQL maintenance decisions.
pub const POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL: &str = "andromeda_postgresql_maintenance_decisions_total";
/// Counter of PostgreSQL maintenance action outcomes.
pub const POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL: &str = "andromeda_postgresql_maintenance_actions_total";
/// Histogram of PostgreSQL maintenance action wall-clock seconds.
pub const POSTGRESQL_MAINTENANCE_ACTION_SECONDS: &str = "andromeda_postgresql_maintenance_action_seconds";
/// Histogram of bytes physically reclaimed by PostgreSQL maintenance actions.
pub const POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES: &str = "andromeda_postgresql_maintenance_reclaimed_bytes";

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
/// The `decision` label naming a maintenance pass result.
pub const LABEL_DECISION: &str = "decision";
/// The `action` label naming a bounded maintenance operation family.
pub const LABEL_ACTION: &str = "action";
