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
/// Histogram of wall-clock seconds spent awaiting one database statement.
pub const DB_STATEMENT_SECONDS: &str = "andromeda_db_statement_seconds";
/// Counter of database statements that returned an error.
pub const DB_STATEMENT_FAILURES_TOTAL: &str = "andromeda_db_statement_failures_total";
/// Gauge of open connections in one tenant's pool.
pub const DB_POOL_CONNECTIONS: &str = "andromeda_db_pool_connections";
/// Gauge of idle connections in one tenant's pool.
pub const DB_POOL_IDLE_CONNECTIONS: &str = "andromeda_db_pool_idle_connections";
/// Gauge of total on-disk bytes for one live-schema base table, indexes and TOAST included.
pub const DB_TABLE_BYTES: &str = "andromeda_db_table_bytes";
/// Gauge of on-disk bytes held by one live-schema base table's indexes.
pub const DB_TABLE_INDEX_BYTES: &str = "andromeda_db_table_index_bytes";
/// Counter of durable-queue item outcomes.
pub const QUEUE_OUTCOMES_TOTAL: &str = "andromeda_queue_outcomes_total";
/// Counter of entity-service operations, labelled by operation and outcome.
pub const ENTITY_OPERATIONS_TOTAL: &str = "andromeda_entity_operations_total";
/// Histogram of entity-service operation durations in seconds, labelled by operation.
pub const ENTITY_OPERATION_SECONDS: &str = "andromeda_entity_operation_seconds";
/// Counter of temporal-service operations, labelled by operation and outcome.
pub const TEMPORAL_OPERATIONS_TOTAL: &str = "andromeda_temporal_operations_total";
/// Histogram of temporal-service operation durations in seconds, labelled by operation.
pub const TEMPORAL_OPERATION_SECONDS: &str = "andromeda_temporal_operation_seconds";
/// Counter of subscription lifecycle operations, labelled by operation and outcome.
pub const SUBSCRIPTION_OPERATIONS_TOTAL: &str = "andromeda_subscription_operations_total";
/// Counter of outbound federation requests, labelled by operation and outcome.
pub const FEDERATION_REQUESTS_TOTAL: &str = "andromeda_federation_requests_total";
/// Histogram of outbound federation request durations in seconds, labelled by operation.
pub const FEDERATION_REQUEST_SECONDS: &str = "andromeda_federation_request_seconds";
/// Counter of upstream HTTP status classes returned to federation requests.
pub const FEDERATION_UPSTREAM_STATUS_TOTAL: &str = "andromeda_federation_upstream_status_total";
/// Counter of notification deliveries, labelled by outcome.
pub const NOTIFICATION_DELIVERIES_TOTAL: &str = "andromeda_notification_deliveries_total";
/// Histogram of notification delivery durations in seconds.
pub const NOTIFICATION_DELIVERY_SECONDS: &str = "andromeda_notification_delivery_seconds";
/// Counter of background task failures, labelled by failure class and criticality.
pub const TASK_FAILURES_TOTAL: &str = "andromeda_task_failures_total";
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
/// Histogram of dead tuples removed by PostgreSQL maintenance vacuums.
pub const POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES: &str = "andromeda_postgresql_maintenance_removed_dead_tuples";
/// Gauge of the oldest database transaction ID age (`age(datfrozenxid)`) across the tenant fleet.
pub const POSTGRESQL_FREEZE_AGE_MAX: &str = "andromeda_postgresql_freeze_age_max";
/// Gauge of the oldest database multixact ID age (`mxid_age(datminmxid)`) across the tenant fleet.
pub const POSTGRESQL_MULTIXACT_AGE_MAX: &str = "andromeda_postgresql_multixact_age_max";
/// Gauge of tenant databases at each freeze pressure level.
pub const POSTGRESQL_DATABASES_FREEZE_PRESSURE: &str = "andromeda_postgresql_databases_freeze_pressure";
/// Gauge of tenant runtimes currently held in the bounded runtime cache.
pub const TENANT_RUNTIMES_CACHED: &str = "andromeda_tenant_runtimes_cached";
/// Counter of cold tenant-runtime builds, labelled by outcome.
pub const TENANT_COLD_BUILDS_TOTAL: &str = "andromeda_tenant_cold_builds_total";
/// Histogram of cold tenant-runtime build durations in seconds.
pub const TENANT_COLD_BUILD_SECONDS: &str = "andromeda_tenant_cold_build_seconds";
/// Histogram of seconds a cold build waited for a build-admission permit.
pub const TENANT_BUILD_ADMISSION_WAIT_SECONDS: &str = "andromeda_tenant_build_admission_wait_seconds";
/// Counter of tenant-runtime evictions, labelled by reason.
pub const TENANT_EVICTIONS_TOTAL: &str = "andromeda_tenant_evictions_total";
/// Counter of dormant-tenant wake attempts, labelled by outcome.
pub const TENANT_WAKES_TOTAL: &str = "andromeda_tenant_wakes_total";

/// The `statement` label naming a database statement.
pub const LABEL_STATEMENT: &str = "statement";
/// The `tenant` label naming the tenant a pool serves.
pub const LABEL_TENANT: &str = "tenant";
/// The `table` label naming a database base table.
pub const LABEL_TABLE: &str = "table";
/// The `queue` label naming a durable queue.
pub const LABEL_QUEUE: &str = "queue";
/// The `outcome` label naming what happened to a queue item.
pub const LABEL_OUTCOME: &str = "outcome";
/// The `reason` label naming why a tenant runtime was evicted.
pub const LABEL_REASON: &str = "reason";
/// The `operation` label naming a domain operation.
pub const LABEL_OPERATION: &str = "operation";
/// The `status` label naming an upstream HTTP status class.
pub const LABEL_STATUS: &str = "status";
/// The `class` label naming a task-failure class.
pub const LABEL_CLASS: &str = "class";
/// The `criticality` label naming a task's operational criticality.
pub const LABEL_CRITICALITY: &str = "criticality";
/// The `stage` label naming a subscription funnel stage.
pub const LABEL_STAGE: &str = "stage";
/// The `decision` label naming a maintenance pass result.
pub const LABEL_DECISION: &str = "decision";
/// The `action` label naming a bounded maintenance operation family.
pub const LABEL_ACTION: &str = "action";
/// The `level` label naming a database freeze pressure level.
pub const LABEL_LEVEL: &str = "level";
