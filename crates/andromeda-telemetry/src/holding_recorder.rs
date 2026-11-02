//! The recorder that keeps every gauge and counter without a tenant label in
//! the exposition for as long as the process runs.
//!
//! The exporter reaps every gauge and counter series left untouched for the
//! idle timeout. That bounds the tenant-labelled series, whose sampling stops
//! when their tenant is unloaded, but a gauge set only when its value changes
//! (how many runtimes are cached, how many names a dictionary holds) or a
//! counter of rare events would leave the exposition while the value it last
//! reported still holds. This recorder remembers every gauge and counter series
//! that carries no tenant label and marks each one updated before every render,
//! so the idle timeout reaps only tenant-labelled series.

use crate::names::LABEL_TENANT;
use dashmap::DashSet;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Level, Metadata, Recorder, SharedString, Unit};
use metrics_exporter_prometheus::{PrometheusHandle, PrometheusRecorder};

/// The metadata a held series is looked up with when it is refreshed. The
/// Prometheus recorder ignores metadata.
const REFRESH_METADATA: Metadata<'static> = Metadata::new(module_path!(), Level::INFO, Some(module_path!()));

/// The Prometheus recorder, and the series it holds past the idle timeout.
#[derive(Debug)]
pub(crate) struct HoldingRecorder {
    /// The recorder that stores and renders every series.
    inner: PrometheusRecorder,
    /// Every gauge series registered without a tenant label. A series enters
    /// on its first registration, so a gauge that was never set is not held.
    held_gauges: DashSet<Key>,
    /// Every counter series registered without a tenant label, held the same
    /// way.
    held_counters: DashSet<Key>,
}

impl HoldingRecorder {
    /// Wrap `inner`, holding no series yet.
    pub(crate) fn new(inner: PrometheusRecorder) -> HoldingRecorder {
        HoldingRecorder {
            inner,
            held_gauges: DashSet::new(),
            held_counters: DashSet::new(),
        }
    }

    /// The handle that renders the recorder's contents.
    pub(crate) fn handle(&self) -> PrometheusHandle {
        self.inner.handle()
    }

    /// Mark every held series as updated without changing its value.
    ///
    /// Adding zero leaves the value as it is and advances the generation the
    /// exporter's idle check compares, so a render that follows keeps the series.
    pub(crate) fn refresh_held(&self) {
        for key in self.held_gauges.iter() {
            self.inner.register_gauge(key.key(), &REFRESH_METADATA).increment(0.0);
        }
        for key in self.held_counters.iter() {
            self.inner.register_counter(key.key(), &REFRESH_METADATA).increment(0);
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
        let counter = self.inner.register_counter(key, metadata);
        hold(&self.held_counters, key);
        counter
    }

    fn register_gauge(&self, key: &Key, metadata: &Metadata<'_>) -> Gauge {
        let gauge = self.inner.register_gauge(key, metadata);
        hold(&self.held_gauges, key);
        gauge
    }

    fn register_histogram(&self, key: &Key, metadata: &Metadata<'_>) -> Histogram {
        self.inner.register_histogram(key, metadata)
    }
}

/// Add `key` to `held` unless it names a series of one tenant.
fn hold(held: &DashSet<Key>, key: &Key) {
    if !carries_tenant_label(key) && !held.contains(key) {
        // clone: the held set owns its keys beyond the registration that named them.
        held.insert(key.clone());
    }
}

/// Whether `key` names a series of one tenant, which the idle timeout reaps
/// once the tenant is unloaded and its sampling stops.
fn carries_tenant_label(key: &Key) -> bool {
    key.labels().any(|label| label.key() == LABEL_TENANT)
}
