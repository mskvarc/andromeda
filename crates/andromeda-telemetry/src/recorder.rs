//! The one Prometheus recorder the process installs, and its rendered output.

use crate::{
    error::TelemetryError,
    names::{DB_STATEMENT_SECONDS, POSTGRESQL_MAINTENANCE_ACTION_SECONDS, POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES, SUBSCRIPTION_CANDIDATES},
};
use metrics::{Recorder, with_local_recorder};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle, PrometheusRecorder};

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
        let handle = builder()?.install_recorder()?;
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
        let recorder = builder()?.build_recorder();
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

/// The builder carrying the broker's histogram bucket configuration.
fn builder() -> Result<PrometheusBuilder, TelemetryError> {
    let builder = PrometheusBuilder::new()
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
        names::{DB_STATEMENT_SECONDS, QUEUE_OUTCOMES_TOTAL},
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

    #[test]
    fn a_detached_recorder_with_nothing_recorded_renders_empty() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        pretty_assertions::assert_eq!(recorder.render().trim(), "");
        Ok(())
    }
}
