//! Startup reporting for the extension configuration.
//!
//! The one message that has to be loud is the conformance override silently
//! disabling a feature the operator turned on: a broker that thinks it enabled
//! an extension but is running the ETSI suite would otherwise fail requests it
//! believes it serves. Everything else about the extension set is ordinary
//! informational logging left to the composition owner.

use andromeda_configuration::schema::extensions::ExtensionsConfig;

/// Warn loudly when the conformance override forced off a feature the
/// operator enabled.
///
/// Called once at startup, after the configuration is validated and before any
/// route is built. It fires only when both are true: conformance mode is on and
/// at least one extension's own table asked for it, which is exactly the case where
/// the effective behaviour diverges from what the configuration appears to say.
pub fn log_conformance_override(config: &ExtensionsConfig) {
    if config.is_conformance() && config.any_feature_requested() {
        tracing::warn!(
            extensions_mode = "conformance",
            "extensions.mode is \"conformance\", forcing every configured extension OFF; \
             the broker is running in conformance mode and will not serve any non-standard extension"
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::startup_log::log_conformance_override;
    use andromeda_configuration::schema::{
        extensions::{ExtensionsConfig, ExtensionsMode, bounded::TemporalDeleteByQueryConfig},
        feature_mode::FeatureMode,
    };

    /// The reporter must not panic in any of the four combinations of the two
    /// conditions it guards on; the message content is a `tracing` concern.
    #[test]
    fn the_override_report_runs_for_every_configuration_shape() {
        for (mode, feature) in [
            (ExtensionsMode::Configured, FeatureMode::Disabled),
            (ExtensionsMode::Configured, FeatureMode::Enabled),
            (ExtensionsMode::Conformance, FeatureMode::Disabled),
            (ExtensionsMode::Conformance, FeatureMode::Enabled),
        ] {
            let config = ExtensionsConfig {
                mode,
                temporal_delete_by_query: TemporalDeleteByQueryConfig {
                    mode: feature,
                    ..TemporalDeleteByQueryConfig::default()
                },
                ..ExtensionsConfig::default()
            };
            log_conformance_override(&config);
        }
    }
}
