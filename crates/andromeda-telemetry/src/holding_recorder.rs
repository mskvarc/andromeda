//! The recorder that keeps every gauge without a tenant label in the
//! exposition for as long as the process runs.
//!
//! The exporter reaps every gauge series left unset for the gauge idle timeout.
//! That bounds the tenant-labelled gauges, whose sampling stops when their
//! tenant is unloaded, but a gauge set only when its value changes (how many
//! runtimes are cached, how many names a dictionary holds) or only on some
//! passes would leave the exposition while the value it last reported still
//! holds. This recorder remembers every gauge series that carries no tenant
//! label and marks each one updated before every render, so the idle timeout
//! reaps only tenant-labelled series.

use crate::names::LABEL_TENANT;
use dashmap::DashSet;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Level, Metadata, Recorder, SharedString, Unit};
use metrics_exporter_prometheus::{PrometheusHandle, PrometheusRecorder};

/// The metadata a held gauge is looked up with when it is refreshed. The
/// Prometheus recorder ignores metadata.
const REFRESH_METADATA: Metadata<'static> = Metadata::new(module_path!(), Level::INFO, Some(module_path!()));

/// The Prometheus recorder, and the gauge series it holds past the idle timeout.
#[derive(Debug)]
pub(crate) struct HoldingRecorder {
    /// The recorder that stores and renders every series.
    inner: PrometheusRecorder,
    /// Every gauge series registered without a tenant label. A series enters
    /// on its first registration, so a gauge that was never set is not held.
    held: DashSet<Key>,
}

impl HoldingRecorder {
    /// Wrap `inner`, holding no gauge yet.
    pub(crate) fn new(inner: PrometheusRecorder) -> HoldingRecorder {
        HoldingRecorder { inner, held: DashSet::new() }
    }

    /// The handle that renders the recorder's contents.
    pub(crate) fn handle(&self) -> PrometheusHandle {
        self.inner.handle()
    }

    /// Mark every held gauge as updated without changing its value.
    ///
    /// Adding zero leaves the value as it is and advances the generation the
    /// exporter's idle check compares, so a render that follows keeps the series.
    pub(crate) fn refresh_held(&self) {
        for key in self.held.iter() {
            self.inner.register_gauge(key.key(), &REFRESH_METADATA).increment(0.0);
        }
    }
}

impl Recorder for HoldingRecorder {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.inner.describe_counter(key, unit, description);
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.inner.describe_gauge(key, unit, description);
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.inner.describe_histogram(key, unit, description);
    }

    fn register_counter(&self, key: &Key, metadata: &Metadata<'_>) -> Counter {
        self.inner.register_counter(key, metadata)
    }

    fn register_gauge(&self, key: &Key, metadata: &Metadata<'_>) -> Gauge {
        let gauge = self.inner.register_gauge(key, metadata);
        if !carries_tenant_label(key) && !self.held.contains(key) {
            // clone: the held set owns its keys beyond the registration that named them.
            self.held.insert(key.clone());
        }
        gauge
    }

    fn register_histogram(&self, key: &Key, metadata: &Metadata<'_>) -> Histogram {
        self.inner.register_histogram(key, metadata)
    }
}

/// Whether `key` names a series of one tenant, which the idle timeout reaps
/// once the tenant is unloaded and its sampling stops.
fn carries_tenant_label(key: &Key) -> bool {
    key.labels().any(|label| label.key() == LABEL_TENANT)
}
