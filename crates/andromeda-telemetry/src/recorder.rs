//! The one Prometheus recorder the process installs, and its rendered output.

use crate::{
    build_info::BrokerVersion,
    description::describe_metrics,
    error::TelemetryError,
    names::{
        ENTITY_OPERATION_DURATION_SECONDS,
        FEDERATION_REQUEST_DURATION_SECONDS,
        HTTP_REQUEST_DURATION_SECONDS,
        NOTIFICATION_DELIVERY_DURATION_SECONDS,
        POSTGRESQL_MAINTENANCE_ACTION_DURATION_SECONDS,
        POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
        POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
        POSTGRESQL_STATEMENT_DURATION_SECONDS,
        SUBSCRIPTION_CANDIDATES,
        TEMPORAL_OPERATION_DURATION_SECONDS,
        TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS,
        TENANT_COLD_BUILD_DURATION_SECONDS,
    },
};
use metrics::{Recorder, with_local_recorder, with_recorder};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle, PrometheusRecorder};
use metrics_util::MetricKindMask;
use std::time::Duration;

/// How long a gauge series is kept after it stops being updated.
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
const HTTP_REQUEST_SECONDS_BUCKETS: [f64; 11] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds, in seconds, for domain-operation latency.
///
/// Shared by the entity, temporal, federation and notification-delivery
/// operation histograms, and by the wait for a tenant build permit.
const OPERATION_SECONDS_BUCKETS: [f64; 12] = [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds, in seconds, for statement latency.
const STATEMENT_SECONDS_BUCKETS: [f64; 14] = [0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// Histogram bucket bounds, in seconds, for loading a tenant from cold: a pool,
/// pending migrations and the tenant's workers, so up to a minute.
const COLD_BUILD_SECONDS_BUCKETS: [f64; 12] = [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];

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

/// Histogram bucket bounds for the dead tuples one vacuum removes.
const DEAD_TUPLE_BUCKETS: [f64; 8] = [100.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0, 10_000_000.0, 100_000_000.0, 1_000_000_000.0];

/// The bucket set of every histogram the broker records.
///
/// The exporter renders a histogram without a bucket set as a summary, so a
/// histogram missing here would change type the day it gains one.
pub(crate) const HISTOGRAM_BUCKETS: [(&str, &[f64]); 12] = [
    (HTTP_REQUEST_DURATION_SECONDS, &HTTP_REQUEST_SECONDS_BUCKETS),
    (ENTITY_OPERATION_DURATION_SECONDS, &OPERATION_SECONDS_BUCKETS),
    (TEMPORAL_OPERATION_DURATION_SECONDS, &OPERATION_SECONDS_BUCKETS),
    (FEDERATION_REQUEST_DURATION_SECONDS, &OPERATION_SECONDS_BUCKETS),
    (NOTIFICATION_DELIVERY_DURATION_SECONDS, &OPERATION_SECONDS_BUCKETS),
    (TENANT_BUILD_ADMISSION_WAIT_DURATION_SECONDS, &OPERATION_SECONDS_BUCKETS),
    (TENANT_COLD_BUILD_DURATION_SECONDS, &COLD_BUILD_SECONDS_BUCKETS),
    (POSTGRESQL_STATEMENT_DURATION_SECONDS, &STATEMENT_SECONDS_BUCKETS),
    (SUBSCRIPTION_CANDIDATES, &CANDIDATE_BUCKETS),
    (POSTGRESQL_MAINTENANCE_ACTION_DURATION_SECONDS, &MAINTENANCE_SECONDS_BUCKETS),
    (POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES, &RECLAIMED_BYTES_BUCKETS),
    (POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES, &DEAD_TUPLE_BUCKETS),
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
    /// The version the build-info gauge names; `None` renders no build info.
    version: Option<BrokerVersion>,
}

impl TelemetryRecorder {
    /// Build the recorder, install it as the process-wide metrics sink and
    /// describe every metric, labelling the build-info gauge with `version`.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets, or when another recorder already
    /// holds the process-wide sink.
    pub fn install(version: BrokerVersion) -> Result<TelemetryRecorder, TelemetryError> {
        let handle = builder(TENANT_GAUGE_IDLE_TIMEOUT)?.install_recorder()?;
        with_recorder(describe_metrics);
        Ok(TelemetryRecorder {
            local: None,
            handle,
            version: Some(version),
        })
    }

    /// Build the recorder without installing it globally.
    ///
    /// Recording reaches it only inside [`TelemetryRecorder::record_locally`].
    /// It renders no build info.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets. Nothing is registered globally, so no
    /// already-installed recorder can make this fail.
    pub fn detached() -> Result<TelemetryRecorder, TelemetryError> {
        Self::detached_with(TENANT_GAUGE_IDLE_TIMEOUT, None)
    }

    /// Build a detached recorder with an explicit gauge idle timeout and
    /// build-info version.
    ///
    /// Behaves exactly like [`TelemetryRecorder::detached`] but lets a test pick
    /// a short timeout to observe the gauge retraction the eviction path relies
    /// on without waiting the production window, or a version to observe the
    /// build-info gauge.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets.
    fn detached_with(gauge_idle_timeout: Duration, version: Option<BrokerVersion>) -> Result<TelemetryRecorder, TelemetryError> {
        let recorder = builder(gauge_idle_timeout)?.build_recorder();
        describe_metrics(&recorder);
        let handle = recorder.handle();
        Ok(TelemetryRecorder {
            local: Some(recorder),
            handle,
            version,
        })
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
    ///
    /// The build-info gauge is set again before each render: it never changes,
    /// so without the refresh the gauge idle timeout would reap it.
    #[must_use]
    pub fn render(&self) -> String {
        if let Some(version) = self.version {
            self.record_locally(|| version.record());
        }
        self.handle.render()
    }
}

/// The builder carrying the broker's histogram bucket configuration and the
/// gauge idle timeout that bounds tenant-labelled series.
fn builder(gauge_idle_timeout: Duration) -> Result<PrometheusBuilder, TelemetryError> {
    HISTOGRAM_BUCKETS.into_iter().try_fold(
        // Reap a per-tenant gauge series once its tenant is evicted and sampling
        // stops, so tenant-labelled gauge cardinality stays bounded.
        PrometheusBuilder::new().idle_timeout(MetricKindMask::GAUGE, Some(gauge_idle_timeout)),
        |builder, (name, buckets)| Ok(builder.set_buckets_for_metric(Matcher::Full(name.to_owned()), buckets)?),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        build_info::BrokerVersion,
        error::TelemetryError,
        names::{BUILD_INFO, POSTGRESQL_STATEMENT_DURATION_SECONDS, QUEUE_OUTCOMES_TOTAL},
        recorder::{HISTOGRAM_BUCKETS, TelemetryRecorder},
    };
    use metrics::{counter, histogram};
    use std::time::Duration;

    #[test]
    fn a_detached_recorder_renders_what_was_recorded_inside_its_scope() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "unit", "outcome" => "completed").increment(2);
            histogram!(POSTGRESQL_STATEMENT_DURATION_SECONDS, "statement" => "unit").record(0.003);
        });
        counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "outside", "outcome" => "completed").increment(1);
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{QUEUE_OUTCOMES_TOTAL}{{queue=\"unit\",outcome=\"completed\"}} 2")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{POSTGRESQL_STATEMENT_DURATION_SECONDS}_bucket{{statement=\"unit\",le=\"0.005\"}} 1")),
            "{rendered}"
        );
        assert!(!rendered.contains("queue=\"outside\""), "{rendered}");
        Ok(())
    }

    /// A configured bucket set renders as an explicit histogram (`_bucket`
    /// series at the declared boundaries); an unconfigured metric renders as a
    /// summary instead.
    #[test]
    fn every_histogram_renders_as_a_histogram_with_its_configured_buckets() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            for (name, _) in HISTOGRAM_BUCKETS {
                histogram!(name).record(1.0);
            }
        });
        let rendered = recorder.render();
        for (name, buckets) in HISTOGRAM_BUCKETS {
            assert!(rendered.contains(&format!("# TYPE {name} histogram")), "{name}: {rendered}");
            assert!(!rendered.contains(&format!("{name}{{quantile=")), "{name}: {rendered}");
            for bound in buckets {
                assert!(rendered.contains(&format!("{name}_bucket{{le=\"{bound}\"}}")), "{name} {bound}: {rendered}");
            }
        }
        Ok(())
    }

    #[test]
    fn a_recorded_metric_renders_with_its_help_line() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "unit", "outcome" => "completed").increment(1));
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("# HELP {QUEUE_OUTCOMES_TOTAL} ")), "{rendered}");
        Ok(())
    }

    #[test]
    fn a_detached_recorder_with_nothing_recorded_renders_empty() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        pretty_assertions::assert_eq!(recorder.render().trim(), "");
        Ok(())
    }

    /// The build-info gauge is never updated by the broker, so it outlives the
    /// gauge idle timeout only because every render sets it again.
    #[test]
    fn the_build_info_gauge_survives_the_gauge_idle_timeout() -> Result<(), TelemetryError> {
        use std::thread::sleep;

        let recorder = TelemetryRecorder::detached_with(Duration::from_millis(50), Some(BrokerVersion::from_static("1.0.0")))?;
        let expected = format!("{BUILD_INFO}{{version=\"1.0.0\"}} 1");
        assert!(recorder.render().contains(&expected), "rendered at once");
        sleep(Duration::from_millis(150));
        let later = recorder.render();
        assert!(later.contains(&expected), "rendered after the idle timeout: {later}");
        Ok(())
    }

    /// The retraction the eviction path relies on: a tenant-labelled gauge that
    /// stops being sampled (as it does once its tenant is evicted) is dropped at
    /// the next render past the gauge idle timeout, so per-tenant cardinality
    /// stays bounded.
    #[test]
    fn an_idle_tenant_gauge_series_is_reaped_after_the_gauge_idle_timeout() -> Result<(), TelemetryError> {
        use crate::pool::{PoolGauges, PoolSample};
        use std::thread::sleep;

        let recorder = TelemetryRecorder::detached_with(Duration::from_millis(50), None)?;
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
