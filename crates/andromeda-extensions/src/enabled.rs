//! The resolved, effective set of enabled extensions.
//!
//! [`ExtensionsConfig`] is what the operator wrote; [`EnabledExtensions`] is
//! what the broker acts on once [`ExtensionsConfig::conformance_mode`] has been
//! folded in. Resolving once at startup and threading the [`Copy`] result to
//! every gate is what keeps the conformance override in one place: no code path
//! reads the raw per-feature field, so none of them can forget the override.

use crate::config::ExtensionsConfig;
use andromeda_http_stack::config::HttpFeatureMode;

/// The extensions that are actually on for this process.
///
/// Produced by [`ExtensionsConfig::resolve`]. `Copy`, so it is threaded to the
/// route builders, the advertisement, and the service bound by value.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EnabledExtensions {
    /// Whether temporal delete-by-query is routed and served.
    temporal_delete_by_query: bool,
    /// Whether entity Purge accepts `datasetId` / `deleteAll`.
    purge_dataset_scope: bool,
}

impl ExtensionsConfig {
    /// Resolve the effective extension set, folding in the conformance override.
    ///
    /// When [`ExtensionsConfig::conformance_mode`] is [`HttpFeatureMode::Enabled`]
    /// every extension is off regardless of its own field, which is the single
    /// point that makes the override impossible to leak past.
    #[must_use]
    pub const fn resolve(&self) -> EnabledExtensions {
        if self.conformance_mode_enabled() {
            return EnabledExtensions {
                temporal_delete_by_query: false,
                purge_dataset_scope: false,
            };
        }
        EnabledExtensions {
            temporal_delete_by_query: matches!(self.temporal_delete_by_query, HttpFeatureMode::Enabled),
            purge_dataset_scope: matches!(self.purge_dataset_scope, HttpFeatureMode::Enabled),
        }
    }
}

impl EnabledExtensions {
    /// The empty set: no extension enabled.
    ///
    /// The set a conformant broker runs with, and the one route-inventory tests
    /// assert the extension routes are absent under.
    #[must_use]
    pub const fn none() -> EnabledExtensions {
        EnabledExtensions {
            temporal_delete_by_query: false,
            purge_dataset_scope: false,
        }
    }

    /// The full set: every extension enabled.
    ///
    /// A convenience for tests and harnesses that exercise the extensions
    /// without constructing an [`ExtensionsConfig`]; the production path always
    /// goes through [`ExtensionsConfig::resolve`].
    #[must_use]
    pub const fn all() -> EnabledExtensions {
        EnabledExtensions {
            temporal_delete_by_query: true,
            purge_dataset_scope: true,
        }
    }

    /// Whether the temporal delete-by-query extension is enabled.
    #[must_use]
    pub const fn temporal_delete_by_query_enabled(&self) -> bool {
        self.temporal_delete_by_query
    }

    /// Whether the entity-Purge dataset-scope extension is enabled.
    #[must_use]
    pub const fn purge_dataset_scope_enabled(&self) -> bool {
        self.purge_dataset_scope
    }

    /// Whether any extension at all is enabled.
    #[must_use]
    pub const fn any_enabled(&self) -> bool {
        self.temporal_delete_by_query || self.purge_dataset_scope
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::ExtensionsConfig, enabled::EnabledExtensions};
    use andromeda_http_stack::config::HttpFeatureMode;

    fn both_on() -> ExtensionsConfig {
        ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Enabled,
            purge_dataset_scope: HttpFeatureMode::Enabled,
            temporal_delete_by_query_max_matches: 100,
        }
    }

    #[test]
    fn the_default_configuration_enables_nothing() {
        let enabled = ExtensionsConfig::default().resolve();
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.any_enabled());
    }

    #[test]
    fn enabled_fields_resolve_to_their_predicates() {
        let enabled = both_on().resolve();
        assert!(enabled.temporal_delete_by_query_enabled());
        assert!(enabled.purge_dataset_scope_enabled());
        assert!(enabled.any_enabled());
    }

    /// The conformance override wins over both feature fields at once, so a
    /// broker started in conformance mode advertises and routes no extension
    /// whatever the operator turned on.
    #[test]
    fn conformance_mode_forces_every_extension_off() {
        let mut config = both_on();
        config.conformance_mode = HttpFeatureMode::Enabled;
        let enabled = config.resolve();
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
    }

    #[test]
    fn the_empty_set_enables_nothing() {
        let enabled = EnabledExtensions::none();
        assert!(!enabled.any_enabled());
    }
}
