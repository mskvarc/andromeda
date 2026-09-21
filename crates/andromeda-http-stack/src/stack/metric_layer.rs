//! The process-wide axum request-metric layer, built at most once.

use axum_prometheus::{PrometheusMetricLayer, PrometheusMetricLayerBuilder};
use std::sync::OnceLock;

/// The process-wide axum request-metric layer, built at most once.
///
/// [`PrometheusMetricLayerBuilder::with_prefix`] sets a global prefix that may be
/// configured only once per process and panics on a second call, so the built
/// layer is memoised here and cloned into every stack the process assembles.
static HTTP_METRIC_LAYER: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Build (once) and clone the axum request-metric layer.
///
/// The layer emits `andromeda_http_requests_*` through the `metrics` facade into
/// whichever recorder the process installed; it registers no route and installs
/// no recorder of its own. Its ignore pattern excludes the scrape endpoint so a
/// scrape does not instrument itself.
pub(crate) fn http_metric_layer() -> PrometheusMetricLayer<'static> {
    HTTP_METRIC_LAYER
        .get_or_init(|| {
            PrometheusMetricLayerBuilder::new()
                .with_prefix("andromeda")
                .with_ignore_pattern("/ngsi-ld/v1/metrics")
                .build()
        })
        .clone()
}
