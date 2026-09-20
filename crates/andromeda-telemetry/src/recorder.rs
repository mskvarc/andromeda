//! The one Prometheus recorder the process installs, and its rendered output.

use crate::{
    error::TelemetryError,
    names::{
        DB_STATEMENT_SECONDS,
        ENTITY_OPERATION_SECONDS,
        FEDERATION_REQUEST_SECONDS,
        HTTP_REQUEST_DURATION_SECONDS,
        NOTIFICATION_DELIVERY_SECONDS,
        POSTGRESQL_MAINTENANCE_ACTION_SECONDS,
        POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
        SUBSCRIPTION_CANDIDATES,
        TEMPORAL_OPERATION_SECONDS,
    },
};
use metrics::{Recorder, with_local_recorder};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle, PrometheusRecorder};
use metrics_util::MetricKindMask;
use std::time::Duration;

/// How long a per-tenant gauge series is kept after it stops being updated.
///
/// The per-tenant pool and storage gauges ([`crate::pool`], [`crate::storage`])
/// carry a tenant label, so a broker holding thousands of tenants would grow
/// one series per tenant without bound. Their sampling workers refresh them far
/// more often than this while a tenant is cached, so a live tenant's series
/// never lapses; once the tenant is evicted and sampling stops, the recorder
/// reaps its series at the next render past this window. This is the retraction
/// the eviction path relies on: it stops sampling, and the idle timeout drops
/// the series. It is comfortably longer than any sampling interval so a brief
/// stall never drops a live tenant's gauges.
const TENANT_GAUGE_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Histogram bucket bounds, in seconds, for HTTP request latency.
///
/// Mirrors `axum_prometheus::utils::SECONDS_DURATION_BUCKETS`, the bucket set
/// the request-metric layer in `andromeda-http-stack` is documented against.
const HTTP_REQUEST_SECONDS_BUCKETS: [f64; 11] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds, in seconds, for domain-operation latency.
///
/// Shared by the entity, temporal, federation and notification-delivery
/// operation histograms.
const OPERATION_SECONDS_BUCKETS: [f64; 12] = [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds, in seconds, for statement latency.
const STATEMENT_SECONDS_BUCKETS: [f64; 14] = [0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds for candidate counts.
const CANDIDATE_BUCKETS: [f64; 10] = [0.0, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 500.0, 1000.0];

/// Histogram bucket bounds, in seconds, for maintenance actions.
const MAINTENANCE_SECONDS_BUCKETS: [f64; 10] = [0.1, 1.0, 5.0, 15.0, 30.0, 60.0, 300.0, 900.0, 1_800.0, 3_600.0];

/// Histogram bucket bounds, in bytes, for physical reclaim.
const RECLAIMED_BYTES_BUCKETS: [f64; 8] = [
    1_048_576.0,
    16_777_216.0,
    67_108_864.0,
    268_435_456.0,
    1_073_741_824.0,
    4_294_967_296.0,
    17_179_869_184.0,
    68_719_476_736.0,
];

/// The installed metrics recorder and the handle that renders it.
///
/// [`TelemetryRecorder::install`] registers the recorder as the process-wide
/// sink so every `metrics` macro in every crate feeds it.
/// [`TelemetryRecorder::detached`] builds the same recorder without touching
/// process state, for tests and embedded use, and routes recording into it
/// through [`TelemetryRecorder::record_locally`].
#[derive(Debug)]
pub struct TelemetryRecorder {
    /// The recorder itself, retained only when it was not installed globally.
    local: Option<PrometheusRecorder>,
    /// Renders the recorder's current contents.
    handle: PrometheusHandle,
}

impl TelemetryRecorder {
    /// Build the recorder and install it as the process-wide metrics sink.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets, or when another recorder already
    /// holds the process-wide sink.
    pub fn install() -> Result<TelemetryRecorder, TelemetryError> {
        let handle = builder(TENANT_GAUGE_IDLE_TIMEOUT)?.install_recorder()?;
        Ok(TelemetryRecorder { local: None, handle })
    }

    /// Build the recorder without installing it globally.
    ///
    /// Recording reaches it only inside [`TelemetryRecorder::record_locally`].
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets. Nothing is registered globally, so no
    /// already-installed recorder can make this fail.
    pub fn detached() -> Result<TelemetryRecorder, TelemetryError> {
        Self::detached_with_gauge_idle_timeout(TENANT_GAUGE_IDLE_TIMEOUT)
    }

    /// Build a detached recorder with an explicit per-tenant gauge idle timeout.
    ///
    /// Behaves exactly like [`TelemetryRecorder::detached`] but lets a test pick
    /// a short timeout to observe the gauge retraction the eviction path relies
    /// on without waiting the production window.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets.
    fn detached_with_gauge_idle_timeout(gauge_idle_timeout: Duration) -> Result<TelemetryRecorder, TelemetryError> {
        let recorder = builder(gauge_idle_timeout)?.build_recorder();
        let handle = recorder.handle();
        Ok(TelemetryRecorder { local: Some(recorder), handle })
    }

    /// Run `f` with this recorder as the metrics sink for the current thread.
    ///
    /// When the recorder was installed globally there is nothing to redirect and
    /// `f` runs unchanged.
    pub fn record_locally<T>(&self, f: impl FnOnce() -> T) -> T {
        match &self.local {
            Some(recorder) => {
                let recorder: &dyn Recorder = recorder;
                with_local_recorder(recorder, f)
            }
            None => f(),
        }
    }

    /// Render every recorded series in the Prometheus text exposition format.
    #[must_use]
    pub fn render(&self) -> String {
        self.handle.render()
    }
}

/// The builder carrying the broker's histogram bucket configuration and the
/// per-tenant gauge idle timeout that bounds tenant-labelled series.
fn builder(gauge_idle_timeout: Duration) -> Result<PrometheusBuilder, TelemetryError> {
    let builder = PrometheusBuilder::new()
        // Reap a per-tenant gauge series once its tenant is evicted and sampling
        // stops, so tenant-labelled gauge cardinality stays bounded.
        .idle_timeout(MetricKindMask::GAUGE, Some(gauge_idle_timeout))
        .set_buckets_for_metric(Matcher::Full(HTTP_REQUEST_DURATION_SECONDS.to_owned()), &HTTP_REQUEST_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(ENTITY_OPERATION_SECONDS.to_owned()), &OPERATION_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(TEMPORAL_OPERATION_SECONDS.to_owned()), &OPERATION_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(FEDERATION_REQUEST_SECONDS.to_owned()), &OPERATION_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(NOTIFICATION_DELIVERY_SECONDS.to_owned()), &OPERATION_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(DB_STATEMENT_SECONDS.to_owned()), &STATEMENT_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(SUBSCRIPTION_CANDIDATES.to_owned()), &CANDIDATE_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(POSTGRESQL_MAINTENANCE_ACTION_SECONDS.to_owned()), &MAINTENANCE_SECONDS_BUCKETS)?
        .set_buckets_for_metric(Matcher::Full(POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES.to_owned()), &RECLAIMED_BYTES_BUCKETS)?;
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{DB_STATEMENT_SECONDS, HTTP_REQUEST_DURATION_SECONDS, QUEUE_OUTCOMES_TOTAL},
        recorder::TelemetryRecorder,
    };
    use metrics::{counter, histogram};

    #[test]
    fn a_detached_recorder_renders_what_was_recorded_inside_its_scope() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "unit", "outcome" => "completed").increment(2);
            histogram!(DB_STATEMENT_SECONDS, "statement" => "unit").record(0.003);
        });
        counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "outside", "outcome" => "completed").increment(1);
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{QUEUE_OUTCOMES_TOTAL}{{queue=\"unit\",outcome=\"completed\"}} 2")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{DB_STATEMENT_SECONDS}_bucket{{statement=\"unit\",le=\"0.005\"}} 1")),
            "{rendered}"
        );
        assert!(!rendered.contains("queue=\"outside\""), "{rendered}");
        Ok(())
    }

    /// A configured bucket set renders as an explicit histogram (`_bucket`
    /// series at the declared boundaries); an unconfigured metric renders as a
    /// summary instead, so the presence of the HTTP boundaries proves the layer
    /// name is registered with its bucket set.
    #[test]
    fn the_http_request_duration_metric_renders_the_configured_buckets() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            histogram!(HTTP_REQUEST_DURATION_SECONDS, "endpoint" => "/probe", "method" => "GET", "status" => "200").record(0.03);
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{HTTP_REQUEST_DURATION_SECONDS}_bucket")), "{rendered}");
        for boundary in ["0.005", "0.025", "0.05", "10"] {
            assert!(rendered.contains(&format!("le=\"{boundary}\"")), "missing bucket {boundary}: {rendered}");
        }
        Ok(())
    }

    #[test]
    fn a_detached_recorder_with_nothing_recorded_renders_empty() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        pretty_assertions::assert_eq!(recorder.render().trim(), "");
        Ok(())
    }

    /// The retraction the eviction path relies on: a tenant-labelled gauge that
    /// stops being sampled (as it does once its tenant is evicted) is dropped at
    /// the next render past the gauge idle timeout, so per-tenant cardinality
    /// stays bounded.
    #[test]
    fn an_idle_tenant_gauge_series_is_reaped_after_the_gauge_idle_timeout() -> Result<(), TelemetryError> {
        use crate::{
            pool::{PoolGauges, PoolSample},
            recorder::TelemetryRecorder,
        };
        use std::{thread::sleep, time::Duration};

        let recorder = TelemetryRecorder::detached_with_gauge_idle_timeout(Duration::from_millis(50))?;
        recorder.record_locally(|| PoolGauges::for_tenant("acme").record(PoolSample { open: 4, idle: 1 }));
        assert!(
            recorder.render().contains("tenant=\"acme\""),
            "the series is present while the tenant is sampled"
        );

        sleep(Duration::from_millis(150));
        let reaped = recorder.render();
        assert!(
            !reaped.contains("tenant=\"acme\""),
            "the idle series was reaped once sampling stopped: {reaped}"
        );
        Ok(())
    }
}
