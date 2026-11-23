//! Startup reporting for the extension configuration.
//!
//! The one message that has to be loud is the conformance override silently
//! disabling a feature the operator turned on: a broker that thinks it enabled
//! an extension but is running the ETSI suite would otherwise fail requests it
//! believes it serves. Everything else about the extension set is ordinary
//! informational logging left to the composition owner.

use crate::{
    advertisement::token,
    enabled::{EnabledExtensions, Extension},
};
use andromeda_configuration::schema::{extensions::ExtensionsConfig, stability::Stability};

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

/// Warn about each extension the operator turned on that is not stable,
/// naming its token.
///
/// Called once at startup, beside [`log_conformance_override`]. An operator who
/// turned on an experimental extension learns that it may change in the next
/// release, and one who turned on a deprecated extension that it goes away.
/// An extension that is on by default is left out, so a broker that sets no
/// extension logs nothing, and under the conformance override nothing is on.
pub fn log_extension_stability(config: &ExtensionsConfig) {
    for extension in reported(config) {
        match extension.stability() {
            Stability::Stable => {}
            Stability::Experimental => tracing::warn!(
                event = "experimental_extension_enabled",
                extension = token(extension),
                "the extension is experimental, so it may change or go away in any release"
            ),
            Stability::Deprecated => tracing::warn!(
                event = "deprecated_extension_enabled",
                extension = token(extension),
                "the extension is deprecated, and goes away in a later major release"
            ),
        }
    }
}

/// The extensions `config` turns on beyond the defaults that are not stable.
fn reported(config: &ExtensionsConfig) -> Vec<Extension> {
    let defaults = EnabledExtensions::from(&ExtensionsConfig::default());
    EnabledExtensions::from(config)
        .extensions()
        .filter(|extension| !defaults.contains(*extension) && extension.stability() != Stability::Stable)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        enabled::Extension,
        startup_log::{log_conformance_override, reported},
    };
    use andromeda_configuration::schema::{
        extensions::{ExtensionsConfig, ExtensionsMode, bounded::TemporalDeleteByQueryConfig, switch::ExtensionSwitch},
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

    /// Cursor pagination is on by default, so only an extension the operator
    /// turned on is reported, and nothing under the conformance override.
    #[test]
    fn only_an_extension_the_operator_turned_on_is_reported() {
        pretty_assertions::assert_eq!(reported(&ExtensionsConfig::default()), Vec::<Extension>::new());
        let config = ExtensionsConfig {
            near_distance_band: ExtensionSwitch::new(FeatureMode::Enabled),
            ..ExtensionsConfig::default()
        };
        pretty_assertions::assert_eq!(reported(&config), vec![Extension::NearDistanceBand]);
        let conformance = ExtensionsConfig {
            mode: ExtensionsMode::Conformance,
            ..config
        };
        pretty_assertions::assert_eq!(reported(&conformance), Vec::<Extension>::new());
    }
}
