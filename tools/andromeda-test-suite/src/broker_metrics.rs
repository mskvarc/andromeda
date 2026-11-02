//! The Prometheus exposition a broker serves on its management listener, for a test whose subject a broker reports
//! only as a metric: how many tenants it built cold, how many it keeps resident, the maintenance decisions it took.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use prometheus_parse::{Scrape, Value};
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
}

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
        let url = config.pooling()?.metrics(role)?;
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
        let name: &'static str = metric.into();
        self.scrape
            .samples
            .iter()
            .filter(|sample| sample.metric == name)
            .filter_map(|sample| match sample.value {
                Value::Counter(value) | Value::Gauge(value) | Value::Untyped(value) => Some(value),
                Value::Histogram(_) | Value::Summary(_) => None,
            })
            .reduce(|total, value| total + value)
    }
}

#[cfg(test)]
mod tests {
    use crate::broker_metrics::{BrokerMetric, MetricsReading};
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
}
