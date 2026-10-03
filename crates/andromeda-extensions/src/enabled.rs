//! The resolved, effective set of enabled extensions.
//!
//! [`ExtensionsConfig`] is what the operator wrote; [`EnabledExtensions`] is
//! what the broker acts on once [`ExtensionsConfig::conformance_mode`] has been
//! folded in. Resolving once at startup and threading the [`Copy`] result to
//! every gate is what keeps the conformance override in one place: no code path
//! reads the raw per-feature field, so none of them can forget the override.

use crate::config::ExtensionsConfig;
use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
use enumset::{EnumSet, EnumSetType};

/// One opt-in, non-standard extension.
///
/// Membership of the enabled set is a typed [`EnumSet`] rather than one Boolean
/// field per extension: the set makes a duplicate selection unrepresentable,
/// grows without accumulating fields, and is a single value every gate tests
/// (the same idiom the HTTP stack uses for its content codings).
#[derive(Debug, EnumSetType)]
pub enum Extension {
    /// Temporal delete-by-query on `DELETE /temporal/entities`.
    TemporalDeleteByQuery,
    /// `datasetId` / `deleteAll` granularity on entity Purge.
    PurgeDatasetScope,
    /// Grouped temporal aggregation (`aggrGroupBy`).
    TemporalAggregationGrouping,
    /// Tenant management on `/tenants` (introspection and suspend/resume).
    TenantManagement,
    /// Opaque, keyset-backed cursor pagination (clause 5.5.9.1).
    CursorPagination,
}

/// The extensions that are actually on for this process.
///
/// Produced by [`ExtensionsConfig::resolve`]. `Copy`, so it is threaded to the
/// route builders, the advertisement, and the service bound by value.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EnabledExtensions {
    /// The enabled extensions.
    enabled: EnumSet<Extension>,
}

impl ExtensionsConfig {
    /// Resolve the effective extension set, folding in the conformance override.
    ///
    /// When [`ExtensionsConfig::conformance_mode`] is [`HttpFeatureMode::Enabled`]
    /// the set is empty regardless of the individual fields, which is the single
    /// point that makes the override impossible to leak past.
    #[must_use]
    pub fn resolve(&self) -> EnabledExtensions {
        if self.conformance_mode_enabled() {
            return EnabledExtensions { enabled: EnumSet::empty() };
        }
        let mut enabled = EnumSet::empty();
        for (mode, extension) in [
            (self.temporal_delete_by_query, Extension::TemporalDeleteByQuery),
            (self.purge_dataset_scope, Extension::PurgeDatasetScope),
            (self.temporal_aggregation_grouping, Extension::TemporalAggregationGrouping),
            (self.tenant_management, Extension::TenantManagement),
            (self.cursor_pagination, Extension::CursorPagination),
        ] {
            if matches!(mode, HttpFeatureMode::Enabled) {
                enabled.insert(extension);
            }
        }
        EnabledExtensions { enabled }
    }
}

impl EnabledExtensions {
    /// The empty set: no extension enabled.
    ///
    /// The set a conformant broker runs with, and the one route-inventory tests
    /// assert the extension routes are absent under.
    #[must_use]
    pub const fn none() -> EnabledExtensions {
        EnabledExtensions { enabled: EnumSet::empty() }
    }

    /// The full set: every extension enabled.
    ///
    /// A convenience for tests and harnesses that exercise the extensions
    /// without constructing an [`ExtensionsConfig`]; the production path always
    /// goes through [`ExtensionsConfig::resolve`].
    #[must_use]
    pub const fn all() -> EnabledExtensions {
        EnabledExtensions { enabled: EnumSet::all() }
    }

    /// Whether the temporal delete-by-query extension is enabled.
    #[must_use]
    pub fn temporal_delete_by_query_enabled(&self) -> bool {
        self.enabled.contains(Extension::TemporalDeleteByQuery)
    }

    /// Whether the entity-Purge dataset-scope extension is enabled.
    #[must_use]
    pub fn purge_dataset_scope_enabled(&self) -> bool {
        self.enabled.contains(Extension::PurgeDatasetScope)
    }

    /// Whether the grouped temporal aggregation extension is enabled.
    #[must_use]
    pub fn temporal_aggregation_grouping_enabled(&self) -> bool {
        self.enabled.contains(Extension::TemporalAggregationGrouping)
    }

    /// Whether the tenant-management extension is enabled.
    #[must_use]
    pub fn tenant_management_enabled(&self) -> bool {
        self.enabled.contains(Extension::TenantManagement)
    }

    /// Whether opaque cursor pagination is enabled.
    #[must_use]
    pub fn cursor_pagination_enabled(&self) -> bool {
        self.enabled.contains(Extension::CursorPagination)
    }

    /// Whether any extension at all is enabled.
    #[must_use]
    pub fn any_enabled(&self) -> bool {
        !self.enabled.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::ExtensionsConfig, enabled::EnabledExtensions};
    use andromeda_http_stack::config::feature_mode::HttpFeatureMode;

    fn both_on() -> ExtensionsConfig {
        ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Enabled,
            purge_dataset_scope: HttpFeatureMode::Enabled,
            temporal_aggregation_grouping: HttpFeatureMode::Enabled,
            tenant_management: HttpFeatureMode::Enabled,
            cursor_pagination: HttpFeatureMode::Enabled,
            temporal_delete_by_query_max_matches: 100,
            temporal_aggregation_grouping_max_groups: 100,
        }
    }

    #[test]
    fn the_default_configuration_enables_cursor_pagination_alone() {
        let enabled = ExtensionsConfig::default().resolve();
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.temporal_aggregation_grouping_enabled());
        assert!(!enabled.tenant_management_enabled());
        assert!(enabled.cursor_pagination_enabled());
    }

    #[test]
    fn enabled_fields_resolve_to_their_predicates() {
        let enabled = both_on().resolve();
        assert!(enabled.temporal_delete_by_query_enabled());
        assert!(enabled.purge_dataset_scope_enabled());
        assert!(enabled.temporal_aggregation_grouping_enabled());
        assert!(enabled.tenant_management_enabled());
        assert!(enabled.cursor_pagination_enabled());
        assert!(enabled.any_enabled());
    }

    /// The conformance override wins over every feature field at once, so a
    /// broker started in conformance mode advertises and routes no extension
    /// whatever the operator turned on.
    #[test]
    fn conformance_mode_forces_every_extension_off() {
        let mut config = both_on();
        config.conformance_mode = HttpFeatureMode::Enabled;
        let enabled = config.resolve();
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.temporal_aggregation_grouping_enabled());
        assert!(!enabled.tenant_management_enabled());
        assert!(!enabled.cursor_pagination_enabled());
    }

    /// The conformance override forces the default-on cursor pagination off
    /// like every other extension.
    #[test]
    fn conformance_mode_forces_cursor_pagination_off() {
        let config = ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Enabled,
            ..ExtensionsConfig::default()
        };
        assert!(!config.resolve().cursor_pagination_enabled());
        assert!(!config.resolve().any_enabled());
    }

    #[test]
    fn the_empty_set_enables_nothing() {
        let enabled = EnabledExtensions::none();
        assert!(!enabled.any_enabled());
    }

    #[test]
    fn the_full_set_enables_everything() {
        let enabled = EnabledExtensions::all();
        assert!(enabled.temporal_delete_by_query_enabled());
        assert!(enabled.purge_dataset_scope_enabled());
        assert!(enabled.temporal_aggregation_grouping_enabled());
        assert!(enabled.tenant_management_enabled());
    }
}
