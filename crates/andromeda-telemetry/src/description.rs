//! The `# HELP` text and unit every metric the broker emits is described with.
//!
//! Describing a metric before it is first recorded lets the exposition carry
//! its help line from the first sample on. The list is the one inventory of
//! the broker's metrics: a metric missing from it renders without help.

use crate::names::{
    BUILD_INFO,
    ENTITY_OPERATION_DURATION_SECONDS,
    ENTITY_OPERATIONS_TOTAL,
    FEDERATION_REQUEST_DURATION_SECONDS,
    FEDERATION_REQUESTS_TOTAL,
    FEDERATION_UPSTREAM_STATUS_TOTAL,
    HTTP_REQUEST_DURATION_SECONDS,
    HTTP_REQUESTS_PENDING,
    HTTP_REQUESTS_TOTAL,
    NAME_DICTIONARY_ENTRIES,
    NAME_DICTIONARY_EVICTIONS_TOTAL,
    NAME_DICTIONARY_INSERTED_TOTAL,
    NAME_DICTIONARY_LOOKUPS_TOTAL,
    NOTIFICATION_DELIVERIES_TOTAL,
    NOTIFICATION_DELIVERY_DURATION_SECONDS,
    POSTGRESQL_AUTOVACUUM_WORKERS_BUSY,
    POSTGRESQL_AUTOVACUUM_WORKERS_MAX,
    POSTGRESQL_FREEZE_AGE_MAX,
    POSTGRESQL_FREEZE_PRESSURE_DATABASES,
    POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES,
    POSTGRESQL_MAINTENANCE_ACTION_DURATION_SECONDS,
    POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
    POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
    POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
    POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
    POSTGRESQL_MULTIXACT_AGE_MAX,
    POSTGRESQL_OVERDUE_RELATIONS,
    POSTGRESQL_POOL_CONNECTIONS,
    POSTGRESQL_POOL_IDLE_CONNECTIONS,
    POSTGRESQL_REPLICATION_SLOT_MAX_RETAINED_WAL_BYTES,
    POSTGRESQL_STATEMENT_DURATION_SECONDS,
    POSTGRESQL_STATEMENT_FAILURES_TOTAL,
    POSTGRESQL_TABLE_BYTES,
    POSTGRESQL_TABLE_HOT_UPDATE_RATIO,
    POSTGRESQL_TABLE_INDEX_BYTES,
    QUEUE_OUTCOMES_TOTAL,
    SUBSCRIPTION_CANDIDATES,
    SUBSCRIPTION_OPERATIONS_TOTAL,
    TASK_FAILURES_TOTAL,
    TEMPORAL_OPERATION_DURATION_SECONDS,
    TEMPORAL_OPERATIONS_TOTAL,
    TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS,
    TENANT_COLD_BUILD_DURATION_SECONDS,
    TENANT_COLD_BUILDS_TOTAL,
    TENANT_EVICTIONS_TOTAL,
    TENANT_REQUESTS_SHED_TOTAL,
    TENANT_RUNTIMES_CACHED,
    TENANT_WAKES_TOTAL,
};
use metrics::{KeyName, Recorder, SharedString, Unit};

/// The Prometheus type a metric is exposed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricKind {
    /// A monotonic count.
    Counter,
    /// A value that goes up and down.
    Gauge,
    /// A distribution over the configured buckets.
    Histogram,
}

/// How one metric is described in the exposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetricDescription {
    /// The metric's name.
    name: &'static str,
    /// The type it is exposed as.
    kind: MetricKind,
    /// The unit its samples are measured in; `None` for a plain count or ratio.
    unit: Option<Unit>,
    /// The `# HELP` text.
    help: &'static str,
}

impl MetricDescription {
    /// A counter of plain events.
    const fn counter(name: &'static str, help: &'static str) -> MetricDescription {
        MetricDescription {
            name,
            kind: MetricKind::Counter,
            unit: None,
            help,
        }
    }

    /// A gauge measured in `unit`, or unitless with `None`.
    const fn gauge(name: &'static str, unit: Option<Unit>, help: &'static str) -> MetricDescription {
        MetricDescription {
            name,
            kind: MetricKind::Gauge,
            unit,
            help,
        }
    }

    /// A histogram measured in `unit`, or unitless with `None`.
    const fn histogram(name: &'static str, unit: Option<Unit>, help: &'static str) -> MetricDescription {
        MetricDescription {
            name,
            kind: MetricKind::Histogram,
            unit,
            help,
        }
    }

    /// Register this description with `recorder`.
    fn describe(self, recorder: &dyn Recorder) {
        let name = KeyName::from_const_str(self.name);
        let help = SharedString::const_str(self.help);
        match self.kind {
            MetricKind::Counter => recorder.describe_counter(name, self.unit, help),
            MetricKind::Gauge => recorder.describe_gauge(name, self.unit, help),
            MetricKind::Histogram => recorder.describe_histogram(name, self.unit, help),
        }
    }
}

/// Every metric the broker emits.
const DESCRIPTIONS: [MetricDescription; 48] = [
    MetricDescription::gauge(BUILD_INFO, None, "Always 1, labelled by the version of the running broker."),
    MetricDescription::counter(HTTP_REQUESTS_TOTAL, "HTTP requests answered, by method, matched endpoint and status code."),
    MetricDescription::histogram(
        HTTP_REQUEST_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken to answer an HTTP request, by method, matched endpoint and status code.",
    ),
    MetricDescription::gauge(HTTP_REQUESTS_PENDING, None, "HTTP requests in progress, by method and matched endpoint."),
    MetricDescription::histogram(
        POSTGRESQL_STATEMENT_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time spent awaiting one database statement, by statement.",
    ),
    MetricDescription::counter(POSTGRESQL_STATEMENT_FAILURES_TOTAL, "Database statements that returned an error, by statement."),
    MetricDescription::gauge(POSTGRESQL_POOL_CONNECTIONS, None, "Open connections in a tenant's pool, idle ones included."),
    MetricDescription::gauge(POSTGRESQL_POOL_IDLE_CONNECTIONS, None, "Idle connections in a tenant's pool."),
    MetricDescription::gauge(
        POSTGRESQL_TABLE_BYTES,
        Some(Unit::Bytes),
        "On-disk size of a tenant table, indexes and TOAST included.",
    ),
    MetricDescription::gauge(POSTGRESQL_TABLE_INDEX_BYTES, Some(Unit::Bytes), "On-disk size of a tenant table's indexes."),
    MetricDescription::gauge(
        POSTGRESQL_TABLE_HOT_UPDATE_RATIO,
        None,
        "Share, from 0 to 1, of a tenant table's updates that were heap-only and touched no index.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_HISTORY_LARGEST_ROWSTORE_CHUNK_BYTES,
        Some(Unit::Bytes),
        "Size of a history table's largest chunk still waiting for columnstore conversion; 0 with nothing to convert.",
    ),
    MetricDescription::counter(QUEUE_OUTCOMES_TOTAL, "Items claimed from a work queue, by queue and what happened to them."),
    MetricDescription::counter(ENTITY_OPERATIONS_TOTAL, "Entity operations, by operation and outcome."),
    MetricDescription::histogram(
        ENTITY_OPERATION_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken by an entity operation, by operation.",
    ),
    MetricDescription::counter(TEMPORAL_OPERATIONS_TOTAL, "Temporal API operations, by operation and outcome."),
    MetricDescription::histogram(
        TEMPORAL_OPERATION_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken by a Temporal API operation, by operation.",
    ),
    MetricDescription::counter(
        SUBSCRIPTION_OPERATIONS_TOTAL,
        "Subscriptions created, updated and deleted, by operation and outcome.",
    ),
    MetricDescription::histogram(
        SUBSCRIPTION_CANDIDATES,
        None,
        "Subscriptions still in play after each matching stage, per change, by stage.",
    ),
    MetricDescription::counter(FEDERATION_REQUESTS_TOTAL, "Requests sent to context sources, by operation and outcome."),
    MetricDescription::histogram(
        FEDERATION_REQUEST_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken by a request sent to a context source, by operation.",
    ),
    MetricDescription::counter(FEDERATION_UPSTREAM_STATUS_TOTAL, "Answers from context sources, by HTTP status class."),
    MetricDescription::counter(NOTIFICATION_DELIVERIES_TOTAL, "Notification delivery attempts, by outcome."),
    MetricDescription::histogram(
        NOTIFICATION_DELIVERY_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken by a notification delivery attempt.",
    ),
    MetricDescription::counter(TASK_FAILURES_TOTAL, "Background tasks that failed for good, by failure class and criticality."),
    MetricDescription::counter(
        POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
        "What each PostgreSQL maintenance pass decided, by decision.",
    ),
    MetricDescription::counter(
        POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
        "PostgreSQL maintenance actions run, by action and outcome.",
    ),
    MetricDescription::histogram(
        POSTGRESQL_MAINTENANCE_ACTION_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time a PostgreSQL maintenance action ran, by action and outcome.",
    ),
    MetricDescription::histogram(
        POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
        Some(Unit::Bytes),
        "Disk space a successful PostgreSQL maintenance rebuild gave back, by action.",
    ),
    MetricDescription::histogram(
        POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
        None,
        "Dead rows a PostgreSQL maintenance vacuum removed, by action.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_FREEZE_AGE_MAX,
        None,
        "Oldest transaction ID age, in transactions, across the tenant databases.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_MULTIXACT_AGE_MAX,
        None,
        "Oldest multixact ID age, in multixacts, across the tenant databases.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_FREEZE_PRESSURE_DATABASES,
        None,
        "Tenant databases at each freeze pressure level, by level.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_AUTOVACUUM_WORKERS_BUSY,
        None,
        "Autovacuum workers running in the PostgreSQL cluster when maintenance last looked.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_AUTOVACUUM_WORKERS_MAX,
        None,
        "The PostgreSQL cluster's autovacuum_max_workers when maintenance last looked.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_OVERDUE_RELATIONS,
        None,
        "Tables overdue for a vacuum with none running, in the latest tenant visit.",
    ),
    MetricDescription::gauge(
        POSTGRESQL_REPLICATION_SLOT_MAX_RETAINED_WAL_BYTES,
        Some(Unit::Bytes),
        "The most WAL any replication slot keeps on disk; 0 without a slot.",
    ),
    MetricDescription::gauge(TENANT_RUNTIMES_CACHED, None, "Tenants loaded right now."),
    MetricDescription::counter(TENANT_COLD_BUILDS_TOTAL, "Tenants loaded from cold, by outcome."),
    MetricDescription::histogram(
        TENANT_COLD_BUILD_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time taken to load a tenant from cold.",
    ),
    MetricDescription::histogram(
        TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS,
        Some(Unit::Seconds),
        "Time a tenant waited for its turn to load.",
    ),
    MetricDescription::counter(
        TENANT_REQUESTS_SHED_TOTAL,
        "Requests answered 503 because their tenant could not be reached or loaded in time, by the wait that shed them.",
    ),
    MetricDescription::counter(TENANT_EVICTIONS_TOTAL, "Tenants unloaded, by reason."),
    MetricDescription::counter(TENANT_WAKES_TOTAL, "Attempts to load a tenant for its scheduled work, by outcome."),
    MetricDescription::counter(
        NAME_DICTIONARY_LOOKUPS_TOTAL,
        "Names looked up in a name dictionary cache, by dictionary and outcome.",
    ),
    MetricDescription::counter(
        NAME_DICTIONARY_EVICTIONS_TOTAL,
        "Names dropped from a name dictionary cache, by dictionary and cause.",
    ),
    MetricDescription::gauge(NAME_DICTIONARY_ENTRIES, None, "Names the loaded name dictionary caches hold, by dictionary."),
    MetricDescription::counter(
        NAME_DICTIONARY_INSERTED_TOTAL,
        "Names added to a name dictionary for the first time, by dictionary.",
    ),
];

/// Register every metric's help text and unit with `recorder`.
pub(crate) fn describe_metrics(recorder: &dyn Recorder) {
    for description in DESCRIPTIONS {
        description.describe(recorder);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        description::{DESCRIPTIONS, MetricKind},
        recorder::HISTOGRAM_BUCKETS,
    };
    use std::collections::HashSet;

    /// Every `pub const` metric name in the names module, read from its source
    /// so a name added there without a description fails here.
    fn declared_metric_names() -> HashSet<&'static str> {
        include_str!("names.rs")
            .lines()
            .filter(|line| line.starts_with("pub const ") && !line.starts_with("pub const LABEL_"))
            .filter_map(|line| line.split('"').nth(1))
            .collect()
    }

    #[test]
    fn every_declared_metric_is_described_exactly_once() {
        let described: Vec<&str> = DESCRIPTIONS.iter().map(|description| description.name).collect();
        let unique: HashSet<&str> = described.iter().copied().collect();
        pretty_assertions::assert_eq!(unique.len(), described.len(), "a metric is described twice");
        pretty_assertions::assert_eq!(unique, declared_metric_names());
    }

    #[test]
    fn every_described_histogram_has_a_bucket_set() {
        let bucketed: HashSet<&str> = HISTOGRAM_BUCKETS.iter().map(|(name, _)| *name).collect();
        let histograms: HashSet<&str> = DESCRIPTIONS
            .iter()
            .filter(|description| description.kind == MetricKind::Histogram)
            .map(|description| description.name)
            .collect();
        pretty_assertions::assert_eq!(histograms, bucketed);
    }

    #[test]
    fn every_help_text_is_one_sentence_ending_in_a_full_stop() {
        for description in DESCRIPTIONS {
            assert!(description.help.ends_with('.'), "{}", description.name);
            assert!(!description.help.contains('\n'), "{}", description.name);
        }
    }
}
