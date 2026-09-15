//! Startup reporting for the extension configuration.
//!
//! The one message that has to be loud is the conformance override silently
//! disabling a feature the operator turned on: a broker that thinks it enabled
//! an extension but is running the ETSI suite would otherwise fail requests it
//! believes it serves. Everything else about the extension set is ordinary
//! informational logging left to the composition owner.

use crate::config::ExtensionsConfig;

/// Warn loudly when `conformance_mode` overrode a feature the operator enabled.
///
/// Called once at startup, after the configuration is validated and before any
/// route is built. It fires only when both are true: conformance mode is on and
/// at least one extension field asked to be on, which is exactly the case where
/// the effective behaviour diverges from what the configuration appears to say.
pub fn log_conformance_override(config: &ExtensionsConfig) {
    if config.conformance_mode_enabled() && config.any_feature_requested() {
        tracing::warn!(
            conformance_mode = "enabled",
            "extensions.conformance_mode is forcing every configured extension OFF; \
             the broker is running in conformance mode and will not serve any non-standard extension"
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::ExtensionsConfig, startup_log::log_conformance_override};
    use andromeda_http_stack::config::HttpFeatureMode;

    /// The reporter must not panic in any of the four combinations of the two
    /// conditions it guards on; the message content is a `tracing` concern.
    #[test]
    fn the_override_report_runs_for_every_configuration_shape() {
        for (conformance, feature) in [
            (HttpFeatureMode::Disabled, HttpFeatureMode::Disabled),
            (HttpFeatureMode::Disabled, HttpFeatureMode::Enabled),
            (HttpFeatureMode::Enabled, HttpFeatureMode::Disabled),
            (HttpFeatureMode::Enabled, HttpFeatureMode::Enabled),
        ] {
            let config = ExtensionsConfig {
                conformance_mode: conformance,
                temporal_delete_by_query: feature,
                purge_dataset_scope: HttpFeatureMode::Disabled,
                temporal_delete_by_query_max_matches: 100,
            };
            log_conformance_override(&config);
        }
    }
}
