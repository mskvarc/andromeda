//! The one Prometheus recorder the process installs, and its rendered output.

use crate::{
    build_info::BrokerVersion,
    description::describe_metrics,
    error::TelemetryError,
    holding_recorder::HoldingRecorder,
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
use metrics::{Recorder, set_global_recorder, with_local_recorder};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use metrics_util::MetricKindMask;
use std::{sync::Arc, time::Duration};

/// How long a tenant-labelled gauge or counter series is kept after it stops
/// being updated.
///
/// The per-tenant pool and storage series ([`crate::pool`], [`crate::storage`])
/// carry a tenant label, so a broker holding thousands of tenants would grow
/// one series per tenant without bound. Their sampling workers refresh them
/// while a tenant is cached, so a live tenant's series never lapses; once the
/// tenant is evicted and sampling stops, the recorder reaps its series at the
/// next render past this window. This is the retraction the eviction path
/// relies on: it stops sampling, and the idle timeout drops the series. It is
/// comfortably longer than any sampling interval so a brief stall never drops a
/// live tenant's series. Every gauge and counter without a tenant label is held
/// past it (see [`crate::holding_recorder`]).
const TENANT_SERIES_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

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

/// Where the `metrics` macros reach a recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecorderScope {
    /// The recorder is the process-wide sink.
    Installed,
    /// The recorder is reached only inside [`TelemetryRecorder::record_locally`].
    Detached,
}

/// The installed metrics recorder and the handle that renders it.
///
/// [`TelemetryRecorder::install`] registers the recorder as the process-wide
/// sink so every `metrics` macro in every crate feeds it.
/// [`TelemetryRecorder::detached`] builds the same recorder without touching
/// process state, for tests and embedded use, and routes recording into it
/// through [`TelemetryRecorder::record_locally`]. Each recorder holds its own
/// gauges, so nothing recorded into one reaches another.
#[derive(Debug)]
pub struct TelemetryRecorder {
    /// The recorder every series is stored in.
    recorder: Arc<HoldingRecorder>,
    /// Whether the recorder is the process-wide sink.
    scope: RecorderScope,
    /// Renders the recorder's current contents.
    handle: PrometheusHandle,
}

impl TelemetryRecorder {
    /// Build the recorder, install it as the process-wide metrics sink and
    /// describe every metric, labelling the build-info gauge with `version`.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets, and
    /// [`TelemetryError::AlreadyInstalled`] when another recorder already holds
    /// the process-wide sink.
    pub fn install(version: BrokerVersion) -> Result<TelemetryRecorder, TelemetryError> {
        let recorder = Arc::new(HoldingRecorder::new(builder(TENANT_SERIES_IDLE_TIMEOUT)?.build_recorder()));
        set_global_recorder(Arc::clone(&recorder)).map_err(|_rejected| TelemetryError::AlreadyInstalled)?;
        describe_metrics(recorder.as_ref());
        version.record();
        Ok(TelemetryRecorder {
            handle: recorder.handle(),
            recorder,
            scope: RecorderScope::Installed,
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
        Self::detached_with(TENANT_SERIES_IDLE_TIMEOUT, None)
    }

    /// Build a detached recorder with an explicit idle timeout and build-info
    /// version.
    ///
    /// Behaves exactly like [`TelemetryRecorder::detached`] but lets a test pick
    /// a short timeout to observe the series retraction the eviction path relies
    /// on without waiting the production window, or a version to observe the
    /// build-info gauge.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryError::Recorder`] when the Prometheus builder rejects
    /// one of the configured bucket sets.
    pub(crate) fn detached_with(idle_timeout: Duration, version: Option<BrokerVersion>) -> Result<TelemetryRecorder, TelemetryError> {
        let recorder = Arc::new(HoldingRecorder::new(builder(idle_timeout)?.build_recorder()));
        describe_metrics(recorder.as_ref());
        let telemetry = TelemetryRecorder {
            handle: recorder.handle(),
            recorder,
            scope: RecorderScope::Detached,
        };
        if let Some(version) = version {
            telemetry.record_locally(|| version.record());
        }
        Ok(telemetry)
    }

    /// Run `f` with this recorder as the metrics sink for the current thread.
    ///
    /// When the recorder was installed globally there is nothing to redirect and
    /// `f` runs unchanged.
    pub fn record_locally<T>(&self, f: impl FnOnce() -> T) -> T {
        match self.scope {
            RecorderScope::Detached => {
                let recorder: &dyn Recorder = self.recorder.as_ref();
                with_local_recorder(recorder, f)
            }
            RecorderScope::Installed => f(),
        }
    }

    /// Render every recorded series in the Prometheus text exposition format.
    ///
    /// Every gauge and counter without a tenant label is marked updated first,
    /// so the idle timeout reaps only the series of a tenant that stopped being
    /// sampled.
    #[must_use]
    pub fn render(&self) -> String {
        self.recorder.refresh_held();
        self.handle.render()
    }
}

/// The builder carrying the broker's histogram bucket configuration and the
/// idle timeout that bounds tenant-labelled series.
fn builder(idle_timeout: Duration) -> Result<PrometheusBuilder, TelemetryError> {
    HISTOGRAM_BUCKETS.into_iter().try_fold(
        // Reap a per-tenant gauge or counter series once its tenant is evicted
        // and sampling stops, so tenant-labelled cardinality stays bounded. Every
        // other gauge and counter is refreshed before each render and never
        // lapses. No histogram carries a tenant label.
        PrometheusBuilder::new().idle_timeout(MetricKindMask::GAUGE | MetricKindMask::COUNTER, Some(idle_timeout)),
        |builder, (name, buckets)| Ok(builder.set_buckets_for_metric(Matcher::Full(name.to_owned()), buckets)?),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        build_info::BrokerVersion,
        error::TelemetryError,
        http::{HttpEndpoint, HttpMethod, HttpRequestObservation},
        maintenance::{FreezePressureLevel, record_autovacuum_workers, record_fleet_freeze, record_overdue_relations, record_replication_slot_wal},
        names::{
            BUILD_INFO,
            HTTP_REQUESTS_PENDING,
            POSTGRESQL_AUTOVACUUM_WORKERS_BUSY,
            POSTGRESQL_AUTOVACUUM_WORKERS_MAX,
            POSTGRESQL_FREEZE_AGE_MAX,
            POSTGRESQL_FREEZE_PRESSURE_DATABASES,
            POSTGRESQL_MULTIXACT_AGE_MAX,
            POSTGRESQL_OVERDUE_RELATIONS,
            POSTGRESQL_POOL_CONNECTIONS,
            POSTGRESQL_REPLICATION_SLOT_MAX_RETAINED_WAL_BYTES,
            POSTGRESQL_STATEMENT_DURATION_SECONDS,
            POSTGRESQL_TABLE_BYTES,
            POSTGRESQL_TABLE_UPDATES_TOTAL,
            QUEUE_OUTCOMES_TOTAL,
            TASK_FAILURES_TOTAL,
            TENANT_RUNTIMES_CACHED,
        },
        pool::{PoolGauges, PoolSample},
        recorder::{HISTOGRAM_BUCKETS, TelemetryRecorder},
        storage::{StorageMetrics, TableSizeSample},
        tenancy::set_runtimes_cached,
        tenant_label::TenantLabel,
    };
    use metrics::{counter, histogram};
    use std::{sync::Arc, thread::sleep, time::Duration};

    /// An idle timeout short enough for a test to wait past.
    const SHORT_IDLE_TIMEOUT: Duration = Duration::from_millis(50);

    /// Three times [`SHORT_IDLE_TIMEOUT`], so a render after it is past the
    /// timeout however the scheduler delays the sleep's start.
    const PAST_THE_IDLE_TIMEOUT: Duration = Duration::from_millis(150);

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

    /// The build-info gauge is set once, when the recorder is built.
    #[test]
    fn the_build_info_gauge_survives_the_idle_timeout() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(SHORT_IDLE_TIMEOUT, Some(BrokerVersion::from_static("1.0.0")))?;
        let expected = format!("{BUILD_INFO}{{version=\"1.0.0\"}} 1");
        assert!(recorder.render().contains(&expected), "rendered at once");
        sleep(PAST_THE_IDLE_TIMEOUT);
        let later = recorder.render();
        assert!(later.contains(&expected), "rendered after the idle timeout: {later}");
        Ok(())
    }

    /// The retraction the eviction path relies on: a tenant-labelled gauge or
    /// counter that stops being sampled (as it does once its tenant is evicted)
    /// is dropped at the next render past the idle timeout, so per-tenant
    /// cardinality stays bounded.
    #[test]
    fn an_idle_tenant_series_is_reaped_after_the_idle_timeout() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(SHORT_IDLE_TIMEOUT, None)?;
        recorder.record_locally(|| {
            PoolGauges::for_tenant(TenantLabel::named("acme")).record(PoolSample { open: 4, idle: 1 });
            StorageMetrics::for_tenant(TenantLabel::named("acme")).record(
                "entities",
                TableSizeSample {
                    total_bytes: 8192,
                    index_bytes: 4096,
                    updated_tuples: 3,
                    hot_updated_tuples: 2,
                },
            );
        });
        let sampled = recorder.render();
        assert!(
            sampled.contains(&format!("{POSTGRESQL_POOL_CONNECTIONS}{{tenant=\"acme\"}}")),
            "the pool series is present while the tenant is sampled: {sampled}"
        );
        assert!(
            sampled.contains(&format!("{POSTGRESQL_TABLE_BYTES}{{tenant=\"acme\",table=\"entities\"}}")),
            "the table series is present while the tenant is sampled: {sampled}"
        );
        assert!(
            sampled.contains(&format!("{POSTGRESQL_TABLE_UPDATES_TOTAL}{{tenant=\"acme\",table=\"entities\"}} 3")),
            "the update counter is present while the tenant is sampled: {sampled}"
        );

        sleep(PAST_THE_IDLE_TIMEOUT);
        let reaped = recorder.render();
        assert!(
            !reaped.contains("tenant=\"acme\""),
            "the idle series was reaped once sampling stopped: {reaped}"
        );
        Ok(())
    }

    /// Each gauge is set once and then left alone, as a gauge set only when its
    /// value changes is in a broker at rest.
    #[test]
    fn every_gauge_without_a_tenant_label_survives_the_idle_timeout() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(SHORT_IDLE_TIMEOUT, None)?;
        recorder.record_locally(|| {
            set_runtimes_cached(3);
            record_overdue_relations(2);
            record_fleet_freeze(260_000_000, 1_000, &[FreezePressureLevel::Overdue]);
            record_autovacuum_workers(1, 3);
            record_replication_slot_wal(4_096);
            drop(HttpRequestObservation::start(
                HttpMethod::Get,
                HttpEndpoint::Route(Arc::from("/ngsi-ld/v1/entities")),
            ));
        });
        sleep(PAST_THE_IDLE_TIMEOUT);
        let rendered = recorder.render();
        for series in [
            format!("{TENANT_RUNTIMES_CACHED} 3"),
            format!("{POSTGRESQL_OVERDUE_RELATIONS} 2"),
            format!("{POSTGRESQL_FREEZE_AGE_MAX} 260000000"),
            format!("{POSTGRESQL_MULTIXACT_AGE_MAX} 1000"),
            format!("{POSTGRESQL_FREEZE_PRESSURE_DATABASES}{{level=\"normal\"}} 0"),
            format!("{POSTGRESQL_FREEZE_PRESSURE_DATABASES}{{level=\"overdue\"}} 1"),
            format!("{POSTGRESQL_FREEZE_PRESSURE_DATABASES}{{level=\"critical\"}} 0"),
            format!("{POSTGRESQL_AUTOVACUUM_WORKERS_BUSY} 1"),
            format!("{POSTGRESQL_AUTOVACUUM_WORKERS_MAX} 3"),
            format!("{POSTGRESQL_REPLICATION_SLOT_MAX_RETAINED_WAL_BYTES} 4096"),
            format!("{HTTP_REQUESTS_PENDING}{{method=\"GET\",endpoint=\"/ngsi-ld/v1/entities\"}} 0"),
        ] {
            assert!(rendered.contains(&format!("{series}\n")), "{series} after the idle timeout: {rendered}");
        }
        Ok(())
    }

    /// Each counter is incremented once and then left alone, as a counter of
    /// rare events is in a broker at rest.
    #[test]
    fn every_counter_without_a_tenant_label_survives_the_idle_timeout() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(SHORT_IDLE_TIMEOUT, None)?;
        recorder.record_locally(|| {
            counter!(QUEUE_OUTCOMES_TOTAL, "queue" => "unit", "outcome" => "stuck").increment(1);
            counter!(TASK_FAILURES_TOTAL, "class" => "permanent", "criticality" => "tenant-fatal").increment(2);
        });
        sleep(PAST_THE_IDLE_TIMEOUT);
        let rendered = recorder.render();
        for series in [
            format!("{QUEUE_OUTCOMES_TOTAL}{{queue=\"unit\",outcome=\"stuck\"}} 1"),
            format!("{TASK_FAILURES_TOTAL}{{class=\"permanent\",criticality=\"tenant-fatal\"}} 2"),
        ] {
            assert!(rendered.contains(&format!("{series}\n")), "{series} after the idle timeout: {rendered}");
        }
        Ok(())
    }

    #[test]
    fn a_gauge_that_was_never_set_is_not_rendered_after_the_idle_timeout() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached_with(SHORT_IDLE_TIMEOUT, None)?;
        recorder.record_locally(|| set_runtimes_cached(1));
        sleep(PAST_THE_IDLE_TIMEOUT);
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{TENANT_RUNTIMES_CACHED} 1")), "{rendered}");
        for absent in [POSTGRESQL_OVERDUE_RELATIONS, POSTGRESQL_FREEZE_AGE_MAX, HTTP_REQUESTS_PENDING, BUILD_INFO] {
            assert!(!rendered.contains(absent), "{absent} was never set: {rendered}");
        }
        Ok(())
    }
}
