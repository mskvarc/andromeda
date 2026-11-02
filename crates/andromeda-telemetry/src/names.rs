//! The metric and label names the broker emits.
//!
//! Names follow the Prometheus conventions and the `OpenMetrics` unit rule:
//! `snake_case`, an `andromeda_` prefix, the base unit as the last part of the
//! name (`_seconds`, `_bytes`) and `_total` on counters only. Every latency
//! histogram ends in `_duration_seconds`, and every PostgreSQL-specific series
//! carries the `andromeda_postgresql_` prefix. Label values are kebab-case and
//! come from the enums in the sibling modules, so no free-form string becomes a
//! series dimension; the HTTP `endpoint` (a route template), the HTTP `status`
//! code and the `tenant` name are the only values outside a closed list.

/// Gauge, always 1, labelled by the broker version.
pub const BUILD_INFO: &str = "andromeda_build_info";
/// Counter of HTTP requests answered, labelled by method, endpoint and status.
pub const HTTP_REQUESTS_TOTAL: &str = "andromeda_http_requests_total";
/// Histogram of HTTP request durations in seconds, labelled by method, endpoint and status.
pub const HTTP_REQUEST_DURATION_SECONDS: &str = "andromeda_http_request_duration_seconds";
/// Gauge of HTTP requests in progress, labelled by method and endpoint.
pub const HTTP_REQUESTS_PENDING: &str = "andromeda_http_requests_pending";
/// Histogram of wall-clock seconds spent awaiting one database statement.
pub const POSTGRESQL_STATEMENT_DURATION_SECONDS: &str = "andromeda_postgresql_statement_duration_seconds";
/// Counter of database statements that returned an error.
pub const POSTGRESQL_STATEMENT_FAILURES_TOTAL: &str = "andromeda_postgresql_statement_failures_total";
/// Gauge of open connections in one tenant's pool.
pub const POSTGRESQL_POOL_CONNECTIONS: &str = "andromeda_postgresql_pool_connections";
/// Gauge of idle connections in one tenant's pool.
pub const POSTGRESQL_POOL_IDLE_CONNECTIONS: &str = "andromeda_postgresql_pool_idle_connections";
/// Gauge of total on-disk bytes for one live-schema base table, indexes and TOAST included.
pub const POSTGRESQL_TABLE_BYTES: &str = "andromeda_postgresql_table_bytes";
/// Gauge of on-disk bytes held by one live-schema base table's indexes.
pub const POSTGRESQL_TABLE_INDEX_BYTES: &str = "andromeda_postgresql_table_index_bytes";
/// Counter of rows one live-schema base table had updated, heap-only updates
/// included, advanced from the statistics PostgreSQL keeps for the table.
pub const POSTGRESQL_TABLE_UPDATES_TOTAL: &str = "andromeda_postgresql_table_updates_total";
/// Counter of the updates of one live-schema base table that were heap-only
/// (touched no index), advanced the same way.
pub const POSTGRESQL_TABLE_HOT_UPDATES_TOTAL: &str = "andromeda_postgresql_table_hot_updates_total";
/// Gauge of total on-disk bytes, indexes and TOAST included, of one history
/// hypertable's largest chunk the columnstore has not yet converted: the chunk
/// whose conversion blocks every history `UPDATE` and `DELETE`, for about 3 to
/// 4 s per GB on `TimescaleDB` 2.31.
pub const POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES: &str = "andromeda_postgresql_history_largest_rowstore_chunk_bytes";
/// Counter of durable-queue item outcomes.
pub const QUEUE_OUTCOMES_TOTAL: &str = "andromeda_queue_outcomes_total";
/// Counter of entity-service operations, labelled by operation and outcome.
pub const ENTITY_OPERATIONS_TOTAL: &str = "andromeda_entity_operations_total";
/// Histogram of entity-service operation durations in seconds, labelled by operation.
pub const ENTITY_OPERATION_DURATION_SECONDS: &str = "andromeda_entity_operation_duration_seconds";
/// Counter of temporal-service operations, labelled by operation and outcome.
pub const TEMPORAL_OPERATIONS_TOTAL: &str = "andromeda_temporal_operations_total";
/// Histogram of temporal-service operation durations in seconds, labelled by operation.
pub const TEMPORAL_OPERATION_DURATION_SECONDS: &str = "andromeda_temporal_operation_duration_seconds";
/// Counter of subscription lifecycle operations, labelled by operation and outcome.
pub const SUBSCRIPTION_OPERATIONS_TOTAL: &str = "andromeda_subscription_operations_total";
/// Counter of outbound federation requests, labelled by operation and outcome.
pub const FEDERATION_REQUESTS_TOTAL: &str = "andromeda_federation_requests_total";
/// Histogram of outbound federation request durations in seconds, labelled by operation.
pub const FEDERATION_REQUEST_DURATION_SECONDS: &str = "andromeda_federation_request_duration_seconds";
/// Counter of the HTTP status classes context sources answered federation requests with.
pub const FEDERATION_UPSTREAM_STATUS_TOTAL: &str = "andromeda_federation_upstream_status_total";
/// Counter of notification deliveries, labelled by outcome.
pub const NOTIFICATION_DELIVERIES_TOTAL: &str = "andromeda_notification_deliveries_total";
/// Histogram of notification delivery durations in seconds.
pub const NOTIFICATION_DELIVERY_DURATION_SECONDS: &str = "andromeda_notification_delivery_duration_seconds";
/// Counter of background task failures, labelled by failure class and criticality.
pub const TASK_FAILURES_TOTAL: &str = "andromeda_task_failures_total";
/// Histogram of subscription candidates surviving each funnel stage per event.
pub const SUBSCRIPTION_CANDIDATES: &str = "andromeda_subscription_candidates";
/// Counter of adaptive PostgreSQL maintenance decisions.
pub const POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL: &str = "andromeda_postgresql_maintenance_decisions_total";
/// Counter of PostgreSQL maintenance action outcomes.
pub const POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL: &str = "andromeda_postgresql_maintenance_actions_total";
/// Histogram of PostgreSQL maintenance action wall-clock seconds.
pub const POSTGRESQL_MAINTENANCE_ACTION_DURATION_SECONDS: &str = "andromeda_postgresql_maintenance_action_duration_seconds";
/// Histogram of bytes physically reclaimed by PostgreSQL maintenance actions.
pub const POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES: &str = "andromeda_postgresql_maintenance_reclaimed_bytes";
/// Histogram of dead tuples removed by PostgreSQL maintenance vacuums.
pub const POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES: &str = "andromeda_postgresql_maintenance_removed_dead_tuples";
/// Gauge of the oldest database transaction ID age (`age(datfrozenxid)`) across the tenant fleet.
pub const POSTGRESQL_FREEZE_AGE_MAX: &str = "andromeda_postgresql_freeze_age_max";
/// Gauge of the oldest database multixact ID age (`mxid_age(datminmxid)`) across the tenant fleet.
pub const POSTGRESQL_MULTIXACT_AGE_MAX: &str = "andromeda_postgresql_multixact_age_max";
/// Gauge of tenant databases at each freeze pressure level.
pub const POSTGRESQL_FREEZE_PRESSURE_DATABASES: &str = "andromeda_postgresql_freeze_pressure_databases";
/// Gauge of autovacuum workers running across the PostgreSQL cluster when a maintenance pass last looked.
pub const POSTGRESQL_AUTOVACUUM_WORKERS_BUSY: &str = "andromeda_postgresql_autovacuum_workers_busy";
/// Gauge of the cluster's `autovacuum_max_workers` when a maintenance pass last looked.
pub const POSTGRESQL_AUTOVACUUM_WORKERS_MAX: &str = "andromeda_postgresql_autovacuum_workers_max";
/// Gauge of relations whose dead tuples exceed twice their autovacuum trigger with no vacuum running, in the most recent tenant visit.
pub const POSTGRESQL_OVERDUE_RELATIONS: &str = "andromeda_postgresql_overdue_relations";
/// Gauge of the most WAL any replication slot keeps on disk, in bytes, when a maintenance pass last looked; zero without a slot.
pub const POSTGRESQL_REPLICATION_SLOT_MAX_RETAINED_WAL_BYTES: &str = "andromeda_postgresql_replication_slot_max_retained_wal_bytes";
/// Gauge of tenant runtimes currently held in the bounded runtime cache.
pub const TENANT_RUNTIMES_CACHED: &str = "andromeda_tenant_runtimes_cached";
/// Counter of cold tenant-runtime builds, labelled by outcome.
pub const TENANT_COLD_BUILDS_TOTAL: &str = "andromeda_tenant_cold_builds_total";
/// Histogram of cold tenant-runtime build durations in seconds.
pub const TENANT_COLD_BUILD_DURATION_SECONDS: &str = "andromeda_tenant_cold_build_duration_seconds";
/// Histogram of seconds a cold build waited for a build-admission permit.
pub const TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS: &str = "andromeda_tenant_build_admission_wait_duration_seconds";
/// Counter of requests shed as saturated while their tenant was reached or built, labelled by the wait that shed them.
pub const TENANT_REQUESTS_SHED_TOTAL: &str = "andromeda_tenant_requests_shed_total";
/// Counter of tenant-runtime evictions, labelled by reason.
pub const TENANT_EVICTIONS_TOTAL: &str = "andromeda_tenant_evictions_total";
/// Counter of dormant-tenant wake attempts, labelled by outcome.
pub const TENANT_WAKES_TOTAL: &str = "andromeda_tenant_wakes_total";
/// Gauge of tenant wakes overdue when the last wake pass ended: deferred for want of a tenant that may be unloaded, or
/// due and left unclaimed by a pass that spent its whole budget.
pub const TENANT_OVERDUE_WAKES: &str = "andromeda_tenant_overdue_wakes";
/// Gauge of the seconds since the oldest overdue tenant wake fell due, when the last wake pass ended; zero with none.
pub const TENANT_OVERDUE_WAKE_MAX_AGE_SECONDS: &str = "andromeda_tenant_overdue_wake_max_age_seconds";
/// Counter of names asked of a name dictionary cache, labelled by dictionary and outcome.
pub const NAME_DICTIONARY_LOOKUPS_TOTAL: &str = "andromeda_name_dictionary_lookups_total";
/// Counter of names evicted from a name dictionary cache, labelled by dictionary and cause.
pub const NAME_DICTIONARY_EVICTIONS_TOTAL: &str = "andromeda_name_dictionary_evictions_total";
/// Gauge of names held by every loaded cache of one name dictionary.
pub const NAME_DICTIONARY_ENTRIES: &str = "andromeda_name_dictionary_entries";
/// Counter of names newly inserted into a name dictionary, labelled by dictionary.
pub const NAME_DICTIONARY_INSERTIONS_TOTAL: &str = "andromeda_name_dictionary_insertions_total";

/// The `version` label naming the broker release.
pub const LABEL_VERSION: &str = "version";
/// The `method` label naming an HTTP request method.
pub const LABEL_METHOD: &str = "method";
/// The `endpoint` label naming the route an HTTP request matched.
pub const LABEL_ENDPOINT: &str = "endpoint";
/// The `statement` label naming a database statement.
pub const LABEL_STATEMENT: &str = "statement";
/// The `tenant` label naming the tenant a series describes, its values spelled
/// by [`crate::tenant_label::TenantLabel`].
pub const LABEL_TENANT: &str = "tenant";
/// The `table` label naming a database base table.
pub const LABEL_TABLE: &str = "table";
/// The `queue` label naming a durable queue.
pub const LABEL_QUEUE: &str = "queue";
/// The `outcome` label naming what happened to an operation or a queue item.
pub const LABEL_OUTCOME: &str = "outcome";
/// The `reason` label naming why a tenant runtime was evicted.
pub const LABEL_REASON: &str = "reason";
/// The `operation` label naming a domain operation.
pub const LABEL_OPERATION: &str = "operation";
/// The `status` label naming an HTTP status code, or a status class for an upstream answer.
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
/// The `dictionary` label naming a name dictionary.
pub const LABEL_DICTIONARY: &str = "dictionary";
/// The `cause` label naming why a cached name was evicted.
pub const LABEL_CAUSE: &str = "cause";
/// The `wait` label naming the wait a saturated tenant request was shed at.
pub const LABEL_WAIT: &str = "wait";
