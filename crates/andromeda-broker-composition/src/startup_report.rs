//! The one-shot startup profile log emitted before broker resources open.

use andromeda_configuration::{
    broker_config::BrokerConfig,
    load::{PRECEDENCE, ignored_setting::IgnoredSetting},
    schema::management::ManagementMode,
};
use std::net::SocketAddr;
use tracing::{info, warn};

/// Report the active construction profile, and every setting the
/// configuration layers write that the broker ignores, before broker
/// resources are opened.
pub(crate) fn report_startup(config: &BrokerConfig, ignored: &[IgnoredSetting]) {
    info!(
        host = %config.server.host,
        port = config.server.port,
        tenancy_mode = %config.tenancy.mode,
        database_transaction_pooled = config.database.is_pooled(),
        notification_delivery_mode = %config.notifications.delivery.mode,
        log_level = %config.observability.log.level,
        management_mode = %config.management.mode,
        prometheus_metrics = %config.observability.metrics.prometheus.mode,
        precedence = PRECEDENCE,
        "Starting Andromeda"
    );
    report_management_plane(config);
    report_ignored_settings(ignored);
}

/// Warn once for each setting a layer writes that the configuration does not
/// define, naming the layer, so a misspelt key is seen although it loads.
fn report_ignored_settings(ignored: &[IgnoredSetting]) {
    for setting in ignored {
        warn!(
            event = "configuration_setting_ignored",
            setting = %setting.path(),
            layer = <&'static str>::from(setting),
            "{setting}"
        );
    }
}

/// Report where the observability endpoints (`/livez`, `/readyz`, `/metrics`)
/// will be served, so the startup log names the concrete listener an operator or
/// scraper should reach.
fn report_management_plane(config: &BrokerConfig) {
    let data_plane = SocketAddr::from((config.server.host, config.server.port));
    match config.management.mode {
        ManagementMode::SeparatePort => info!(
            data_plane = %data_plane,
            management_plane = %SocketAddr::from((config.management.host, config.management.port)),
            prometheus_metrics = %config.observability.metrics.prometheus.mode,
            "Management plane on a separate port serves /livez, /readyz and, when enabled, /metrics"
        ),
        ManagementMode::SharedPort => info!(
            data_plane = %data_plane,
            prometheus_metrics = %config.observability.metrics.prometheus.mode,
            "Management plane shares the data-plane port at /livez, /readyz and, when enabled, /metrics"
        ),
        ManagementMode::Disabled => info!(
            data_plane = %data_plane,
            "Management plane disabled; data-plane probes remain at /ngsi-ld/v1/health, /ready and /live"
        ),
    }
}
