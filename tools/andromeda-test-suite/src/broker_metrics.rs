//! The Prometheus exposition a broker serves on its management listener, for a test whose subject a broker reports
//! only as a metric: how many tenants it built cold, how many it keeps resident, the maintenance decisions it took, or
//! whether a series is exposed at all.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use prometheus_parse::{Sample, Scrape, Value};
use std::io;
use strum::IntoStaticStr;

/// A metric a test reads, named as the broker exposes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
pub enum BrokerMetric {
    /// The tenant runtimes the broker built, over every outcome.
    #[strum(serialize = "andromeda_tenant_cold_builds_total")]
    ColdBuilds,
    /// The tenant runtimes the broker keeps resident.
    #[strum(serialize = "andromeda_tenant_runtimes_cached")]
    RuntimesCached,
    /// The maintenance decisions the broker took, over every outcome.
    #[strum(serialize = "andromeda_postgresql_maintenance_decisions_total")]
    MaintenanceDecisions,
    /// The maintenance actions the broker ran, by action and outcome.
    #[strum(serialize = "andromeda_postgresql_maintenance_actions_total")]
    MaintenanceActions,
    /// The names the loaded tenants' name dictionary caches hold, by dictionary.
    #[strum(serialize = "andromeda_name_dictionary_entries")]
    NameDictionaryEntries,
    /// The requests in progress, by method and endpoint.
    #[strum(serialize = "andromeda_http_requests_pending")]
    HttpRequestsPending,
    /// The on-disk bytes of a tenant's base tables, by tenant and table.
    #[strum(serialize = "andromeda_postgresql_table_bytes")]
    TableBytes,
    /// The on-disk bytes of a tenant's base-table indexes, by tenant and table.
    #[strum(serialize = "andromeda_postgresql_table_index_bytes")]
    TableIndexBytes,
    /// The rows a tenant's base table had updated, by tenant and table.
    #[strum(serialize = "andromeda_postgresql_table_updates_total")]
    TableUpdates,
    /// The heap-only updates among them, by tenant and table.
    #[strum(serialize = "andromeda_postgresql_table_hot_updates_total")]
    TableHotUpdates,
    /// The largest history chunk of a tenant still in the rowstore, by tenant and hypertable.
    #[strum(serialize = "andromeda_postgresql_history_largest_rowstore_chunk_bytes")]
    HistoryLargestRowstoreChunkBytes,
    /// The tenant wakes overdue as the broker's last wake pass ended.
    #[strum(serialize = "andromeda_tenant_overdue_wakes")]
    OverdueWakes,
    /// The seconds since the oldest overdue tenant wake fell due, as the broker's last wake pass ended.
    #[strum(serialize = "andromeda_tenant_overdue_wake_max_age_seconds")]
    OverdueWakeMaxAgeSeconds,
    /// The notifications refused before sending because they exceed a size limit, by the limit that refused them.
    #[strum(serialize = "andromeda_notification_oversized_total")]
    NotificationsOversized,
}

/// A label a test selects samples by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum MetricLabel {
    /// The tenant a series describes: the tenant name, or [`DEFAULT_TENANT_LABEL`].
    Tenant,
    /// The route template an HTTP series describes, such as `/ngsi-ld/v1/entities`.
    Endpoint,
    /// The size limit a refused notification exceeded: `max-http-body`, `max-mqtt-packet` or `mqtt-server-announced`.
    Limit,
    /// The maintenance action a series counts: `reindex`, `vacuum`, `analyze`, `recovery`, `recompress` or `repack`.
    Action,
}

/// The `tenant` label value of the default tenant's series.
pub const DEFAULT_TENANT_LABEL: &str = "@default";

/// Why the exposition could not be read.
#[derive(Debug, thiserror::Error)]
pub enum BrokerMetricsError {
    /// The stack names no exposition for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The exposition could not be fetched.
    #[error("the metrics exposition could not be fetched")]
    Fetch(#[from] reqwest::Error),
    /// The exposition is not the Prometheus text format.
    #[error("the metrics exposition does not parse")]
    Parse(#[from] io::Error),
}

/// One reading of a broker's exposition.
#[derive(Debug, Clone)]
pub struct MetricsReading {
    /// The samples read.
    scrape: Scrape,
}

impl MetricsReading {
    /// The exposition of the broker of `role`, read now.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerMetricsError`] when the exposition cannot be fetched or does not parse: an unreadable exposition
    /// never reads as zero.
    pub async fn of(role: BrokerRole) -> Result<MetricsReading, BrokerMetricsError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        let url = config.metrics(role)?;
        let text = reqwest::Client::builder()
            .no_proxy()
            .build()?
            .get(url.as_str())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        MetricsReading::parse(&text)
    }

    /// The reading of the exposition `text`.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerMetricsError::Parse`] when the text does not parse.
    pub fn parse(text: &str) -> Result<MetricsReading, BrokerMetricsError> {
        let scrape = Scrape::parse(text.lines().map(|line| Ok(line.to_owned())))?;
        Ok(MetricsReading { scrape })
    }

    /// The sum of every sample of `metric` over its label sets, or `None` when the exposition holds none: a counter
    /// is exposed once it was first incremented.
    #[must_use]
    pub fn sum(&self, metric: BrokerMetric) -> Option<f64> {
        self.sum_where(metric, |_| true)
    }

    /// The sum of the samples of `metric` whose `label` reads `value`, as the exposition spells it, or `None` when
    /// the exposition holds none.
    #[must_use]
    pub fn sum_labelled(&self, metric: BrokerMetric, label: MetricLabel, value: &str) -> Option<f64> {
        let label: &'static str = label.into();
        self.sum_where(metric, |sample| sample.labels.get(label) == Some(value))
    }

    /// The sum of the samples of `metric` that `selected` keeps, or `None` when it keeps none.
    fn sum_where(&self, metric: BrokerMetric, selected: impl Fn(&Sample) -> bool) -> Option<f64> {
        let name: &'static str = metric.into();
        self.scrape
            .samples
            .iter()
            .filter(|sample| sample.metric == name && selected(sample))
            .filter_map(|sample| match sample.value {
                Value::Counter(value) | Value::Gauge(value) | Value::Untyped(value) => Some(value),
                Value::Histogram(_) | Value::Summary(_) => None,
            })
            .reduce(|total, value| total + value)
    }
}

#[cfg(test)]
mod tests {
    use crate::broker_metrics::{BrokerMetric, DEFAULT_TENANT_LABEL, MetricLabel, MetricsReading};
    use googletest::prelude::*;

    #[gtest]
    fn a_counter_sums_over_its_labels_and_an_absent_one_reads_none() -> Result<()> {
        let reading = MetricsReading::parse(
            "# TYPE andromeda_tenant_cold_builds_total counter\n\
             andromeda_tenant_cold_builds_total{outcome=\"ok\"} 3\n\
             andromeda_tenant_cold_builds_total{outcome=\"error\"} 1\n\
             # TYPE andromeda_tenant_runtimes_cached gauge\n\
             andromeda_tenant_runtimes_cached 32\n",
        )?;
        expect_that!(reading.sum(BrokerMetric::ColdBuilds), some(approx_eq(4.0)));
        expect_that!(reading.sum(BrokerMetric::RuntimesCached), some(approx_eq(32.0)));
        expect_that!(reading.sum(BrokerMetric::MaintenanceDecisions), none());
        Ok(())
    }

    #[gtest]
    fn a_labelled_sum_counts_only_the_samples_whose_label_reads_the_value() -> Result<()> {
        let reading = MetricsReading::parse(
            "# TYPE andromeda_postgresql_table_bytes gauge
             andromeda_postgresql_table_bytes{tenant=\"@default\",table=\"entities\"} 8192
             andromeda_postgresql_table_bytes{tenant=\"@default\",table=\"subscriptions\"} 4096
             andromeda_postgresql_table_bytes{tenant=\"acme\",table=\"entities\"} 1024
",
        )?;
        expect_that!(
            reading.sum_labelled(BrokerMetric::TableBytes, MetricLabel::Tenant, DEFAULT_TENANT_LABEL),
            some(approx_eq(12_288.0))
        );
        expect_that!(
            reading.sum_labelled(BrokerMetric::TableBytes, MetricLabel::Tenant, "acme"),
            some(approx_eq(1_024.0))
        );
        expect_that!(reading.sum_labelled(BrokerMetric::TableBytes, MetricLabel::Tenant, "other"), none());
        expect_that!(reading.sum_labelled(BrokerMetric::TableBytes, MetricLabel::Endpoint, "acme"), none());
        expect_that!(reading.sum_labelled(BrokerMetric::TableIndexBytes, MetricLabel::Tenant, "acme"), none());
        Ok(())
    }
}
