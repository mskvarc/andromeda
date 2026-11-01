//! The resolved, effective set of enabled extensions.
//!
//! [`ExtensionsConfig`] is what the operator wrote; [`EnabledExtensions`] is
//! what the broker acts on once the conformance override of
//! [`ExtensionsConfig::mode`] has been folded in. Resolving once at startup and threading the [`Copy`] result to
//! every gate is what keeps the conformance override in one place: no code path
//! reads the raw per-feature field, so none of them can forget the override.

use andromeda_configuration::schema::extensions::{ExtensionsConfig, QTargetLanguages};
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
use enumset::{EnumSet, EnumSetType, enum_set};

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
    /// Linked Entity retrieval (`join`) on the temporal operations.
    TemporalLinkedRetrieval,
    /// Tenant management on `/tenants` (introspection and suspend/resume).
    TenantManagement,
    /// Opaque, keyset-backed cursor pagination (clause 5.5.9.1).
    CursorPagination,
    /// A `near` geo-query bounded by both `maxDistance` and `minDistance`.
    NearDistanceBand,
    /// `Accept-Language` read as the language filter where `lang` is absent.
    AcceptLanguage,
    /// Every RFC 5646 language tag as a `q` target language.
    LanguageTags,
    /// RFC 4647 extended ranges as `q` target languages, and extended
    /// filtering of every target language.
    LanguageRangeFiltering,
}

/// The extensions that are actually on for this process.
///
/// Produced from an [`ExtensionsConfig`]. `Copy`, so it is threaded to the
/// route builders, the advertisement, and the service bound by value.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EnabledExtensions {
    /// The enabled extensions.
    enabled: EnumSet<Extension>,
}

// Under the conformance override the set is empty whatever each extension's
// own table says, which is the single point that makes the override
// impossible to leak past.
impl From<&ExtensionsConfig> for EnabledExtensions {
    fn from(config: &ExtensionsConfig) -> EnabledExtensions {
        if config.is_conformance() {
            return EnabledExtensions { enabled: EnumSet::empty() };
        }
        let mut enabled = EnumSet::empty();
        for (requested, extension) in [
            (config.temporal_delete_by_query.mode.is_enabled(), Extension::TemporalDeleteByQuery),
            (config.purge_dataset_scope.is_requested(), Extension::PurgeDatasetScope),
            (config.temporal_aggregation_grouping.mode.is_enabled(), Extension::TemporalAggregationGrouping),
            (config.temporal_linked_retrieval.is_requested(), Extension::TemporalLinkedRetrieval),
            (config.tenant_management.is_requested(), Extension::TenantManagement),
            (config.cursor_pagination.mode.is_enabled(), Extension::CursorPagination),
            (config.near_distance_band.is_requested(), Extension::NearDistanceBand),
            (config.accept_language.is_requested(), Extension::AcceptLanguage),
        ] {
            if requested {
                enabled.insert(extension);
            }
        }
        EnabledExtensions {
            enabled: enabled | target_language_extensions(QTargetLanguages::from(&config.q_target_languages)),
        }
    }
}

/// The extensions one `q_target_languages` level turns on: each level serves
/// everything the one below it does.
const fn target_language_extensions(level: QTargetLanguages) -> EnumSet<Extension> {
    match level {
        QTargetLanguages::Specification => EnumSet::empty(),
        QTargetLanguages::LanguageTags => enum_set!(Extension::LanguageTags),
        QTargetLanguages::LanguageRanges => enum_set!(Extension::LanguageTags | Extension::LanguageRangeFiltering),
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
    /// goes through the conversion from [`ExtensionsConfig`].
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

    /// Whether Linked Entity retrieval on the temporal operations is enabled.
    #[must_use]
    pub fn temporal_linked_retrieval_enabled(&self) -> bool {
        self.enabled.contains(Extension::TemporalLinkedRetrieval)
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

    /// Whether a `near` geo-query may carry both distances.
    #[must_use]
    pub fn near_distance_band_enabled(&self) -> bool {
        self.enabled.contains(Extension::NearDistanceBand)
    }

    /// Whether `Accept-Language` stands in for an absent `lang`.
    #[must_use]
    pub fn accept_language_enabled(&self) -> bool {
        self.enabled.contains(Extension::AcceptLanguage)
    }

    /// Whether a `q` target language may be any RFC 5646 tag.
    #[must_use]
    pub fn language_tags_enabled(&self) -> bool {
        self.enabled.contains(Extension::LanguageTags)
    }

    /// Whether the target languages of `q` are RFC 4647 extended ranges.
    #[must_use]
    pub fn language_range_filtering_enabled(&self) -> bool {
        self.enabled.contains(Extension::LanguageRangeFiltering)
    }

    /// What a `q` bracket may name as a target language on this broker.
    #[must_use]
    pub fn language_targeting(&self) -> LanguageTargeting {
        if self.language_range_filtering_enabled() {
            LanguageTargeting::LanguageRanges
        } else if self.language_tags_enabled() {
            LanguageTargeting::LanguageTags
        } else {
            LanguageTargeting::Specification
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::enabled::EnabledExtensions;
    use andromeda_configuration::schema::{
        extensions::{
            ExtensionsConfig,
            ExtensionsMode,
            QTargetLanguages,
            QTargetLanguagesConfig,
            bounded::{TemporalAggregationGroupingConfig, TemporalDeleteByQueryConfig},
            cursor_pagination::{CursorPaginationConfig, CursorPaginationMode},
            switch::ExtensionSwitch,
        },
        feature_mode::FeatureMode,
    };
    use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;

    fn both_on() -> ExtensionsConfig {
        ExtensionsConfig {
            mode: ExtensionsMode::Configured,
            temporal_delete_by_query: TemporalDeleteByQueryConfig {
                mode: FeatureMode::Enabled,
                ..TemporalDeleteByQueryConfig::default()
            },
            purge_dataset_scope: ExtensionSwitch::new(FeatureMode::Enabled),
            temporal_aggregation_grouping: TemporalAggregationGroupingConfig {
                mode: FeatureMode::Enabled,
                ..TemporalAggregationGroupingConfig::default()
            },
            temporal_linked_retrieval: ExtensionSwitch::new(FeatureMode::Enabled),
            tenant_management: ExtensionSwitch::new(FeatureMode::Enabled),
            cursor_pagination: CursorPaginationConfig::from(CursorPaginationMode::Default),
            near_distance_band: ExtensionSwitch::new(FeatureMode::Enabled),
            accept_language: ExtensionSwitch::new(FeatureMode::Enabled),
            q_target_languages: QTargetLanguagesConfig::from(QTargetLanguages::LanguageRanges),
        }
    }

    #[test]
    fn the_default_configuration_enables_cursor_pagination_alone() {
        let enabled = EnabledExtensions::from(&ExtensionsConfig::default());
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.temporal_aggregation_grouping_enabled());
        assert!(!enabled.temporal_linked_retrieval_enabled());
        assert!(!enabled.tenant_management_enabled());
        assert!(enabled.cursor_pagination_enabled());
        assert!(!enabled.near_distance_band_enabled());
        assert!(!enabled.accept_language_enabled());
        assert!(!enabled.language_tags_enabled());
        assert!(!enabled.language_range_filtering_enabled());
    }

    #[test]
    fn enabled_fields_resolve_to_their_predicates() {
        let enabled = EnabledExtensions::from(&both_on());
        assert!(enabled.temporal_delete_by_query_enabled());
        assert!(enabled.purge_dataset_scope_enabled());
        assert!(enabled.temporal_aggregation_grouping_enabled());
        assert!(enabled.temporal_linked_retrieval_enabled());
        assert!(enabled.tenant_management_enabled());
        assert!(enabled.cursor_pagination_enabled());
        assert!(enabled.near_distance_band_enabled());
        assert!(enabled.accept_language_enabled());
        assert!(enabled.language_tags_enabled());
        assert!(enabled.language_range_filtering_enabled());
    }

    /// The conformance override wins over every feature field at once, so a
    /// broker started in conformance mode advertises and routes no extension
    /// whatever the operator turned on.
    #[test]
    fn conformance_mode_forces_every_extension_off() {
        let mut config = both_on();
        config.mode = ExtensionsMode::Conformance;
        let enabled = EnabledExtensions::from(&config);
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.temporal_aggregation_grouping_enabled());
        assert!(!enabled.temporal_linked_retrieval_enabled());
        assert!(!enabled.tenant_management_enabled());
        assert!(!enabled.cursor_pagination_enabled());
        assert!(!enabled.near_distance_band_enabled());
        assert!(!enabled.accept_language_enabled());
        assert!(!enabled.language_tags_enabled());
        assert!(!enabled.language_range_filtering_enabled());
    }

    /// Clause 5.7.3.4 and 5.7.4.4 refuse Linked Entity retrieval on the
    /// temporal operations, so conformance mode switches the temporal join
    /// off even when the operator enabled it.
    #[test]
    fn conformance_mode_forces_temporal_linked_retrieval_off() {
        let config = ExtensionsConfig {
            mode: ExtensionsMode::Conformance,
            temporal_linked_retrieval: ExtensionSwitch::new(FeatureMode::Enabled),
            ..ExtensionsConfig::default()
        };
        assert!(!EnabledExtensions::from(&config).temporal_linked_retrieval_enabled());
    }

    /// The conformance override forces the default-on cursor pagination off
    /// like every other extension.
    #[test]
    fn conformance_mode_forces_cursor_pagination_off() {
        let config = ExtensionsConfig {
            mode: ExtensionsMode::Conformance,
            ..ExtensionsConfig::default()
        };
        assert!(!EnabledExtensions::from(&config).cursor_pagination_enabled());
    }

    #[test]
    fn each_target_language_level_serves_the_levels_below_it() {
        for (level, targeting) in [
            (QTargetLanguages::Specification, LanguageTargeting::Specification),
            (QTargetLanguages::LanguageTags, LanguageTargeting::LanguageTags),
            (QTargetLanguages::LanguageRanges, LanguageTargeting::LanguageRanges),
        ] {
            let config = ExtensionsConfig {
                q_target_languages: QTargetLanguagesConfig::from(level),
                ..ExtensionsConfig::default()
            };
            pretty_assertions::assert_eq!(EnabledExtensions::from(&config).language_targeting(), targeting, "{level:?}");
        }
        let tags = EnabledExtensions::from(&ExtensionsConfig {
            q_target_languages: QTargetLanguagesConfig::from(QTargetLanguages::LanguageTags),
            ..ExtensionsConfig::default()
        });
        assert!(tags.language_tags_enabled());
        assert!(!tags.language_range_filtering_enabled());
    }

    /// Clause 4.9 admits no `-` in a bracket, so conformance mode keeps the
    /// specification's grammar whatever level the operator chose.
    #[test]
    fn conformance_mode_forces_the_specification_target_languages() {
        let config = ExtensionsConfig {
            mode: ExtensionsMode::Conformance,
            q_target_languages: QTargetLanguagesConfig::from(QTargetLanguages::LanguageRanges),
            ..ExtensionsConfig::default()
        };
        pretty_assertions::assert_eq!(EnabledExtensions::from(&config).language_targeting(), LanguageTargeting::Specification);
    }

    #[test]
    fn the_empty_set_enables_nothing() {
        let enabled = EnabledExtensions::none();
        assert!(!enabled.temporal_delete_by_query_enabled());
        assert!(!enabled.purge_dataset_scope_enabled());
        assert!(!enabled.temporal_aggregation_grouping_enabled());
        assert!(!enabled.temporal_linked_retrieval_enabled());
        assert!(!enabled.tenant_management_enabled());
        assert!(!enabled.cursor_pagination_enabled());
        assert!(!enabled.near_distance_band_enabled());
        assert!(!enabled.accept_language_enabled());
        assert!(!enabled.language_tags_enabled());
        assert!(!enabled.language_range_filtering_enabled());
    }

    #[test]
    fn the_full_set_enables_everything() {
        let enabled = EnabledExtensions::all();
        assert!(enabled.temporal_delete_by_query_enabled());
        assert!(enabled.purge_dataset_scope_enabled());
        assert!(enabled.temporal_aggregation_grouping_enabled());
        assert!(enabled.temporal_linked_retrieval_enabled());
        assert!(enabled.tenant_management_enabled());
        assert!(enabled.near_distance_band_enabled());
        assert!(enabled.accept_language_enabled());
        assert!(enabled.language_tags_enabled());
        assert!(enabled.language_range_filtering_enabled());
    }
}
