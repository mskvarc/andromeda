//! The one-shot startup profile log emitted before broker resources open.

use crate::config::{management::ManagementMode, runtime::RuntimeConfig};
use std::net::SocketAddr;
use tracing::info;

/// Report the active construction profile before broker resources are opened.
pub(crate) fn report_startup(config: &RuntimeConfig) {
    info!(
        host = %config.server.host,
        port = config.server.port,
        tenancy_enabled = config.tenancy.enabled,
        database_transaction_pooled = config.database.is_pooled(),
        notification_delivery_mode = %config.notifications.delivery.mode,
        log_level = %config.observability.log_level,
        management_mode = %config.management.mode,
        metrics = %config.management.metrics,
        precedence = RuntimeConfig::precedence_summary(),
        "Starting Andromeda"
    );
    report_management_plane(config);
}

/// Report where the observability endpoints (`/livez`, `/readyz`, `/metrics`)
/// will be served, so the startup log names the concrete listener an operator or
/// scraper should reach.
fn report_management_plane(config: &RuntimeConfig) {
    let data_plane = SocketAddr::from((config.server.host, config.server.port));
    match config.management.mode {
        ManagementMode::SeparatePort => info!(
            data_plane = %data_plane,
            management_plane = %SocketAddr::from((config.management.host, config.management.port)),
            metrics = %config.management.metrics,
            "Management plane on a separate port serves /livez, /readyz and, when enabled, /metrics"
        ),
        ManagementMode::Merged => info!(
            data_plane = %data_plane,
            metrics = %config.management.metrics,
            "Management plane merged onto the data-plane port at /livez, /readyz and, when enabled, /metrics"
        ),
        ManagementMode::Disabled => info!(
            data_plane = %data_plane,
            "Management plane disabled; data-plane probes remain at /ngsi-ld/v1/health, /ready and /live"
        ),
    }
}
