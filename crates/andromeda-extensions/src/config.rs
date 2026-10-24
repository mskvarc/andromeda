//! Operator configuration for Andromeda's opt-in, non-standard extensions.
//!
//! Every extension is an additive feature: inert unless a client asks for it, and
//! its "off" state is exactly the behaviour of a conformant broker that never
//! implemented it. The extensions are therefore configured at runtime rather
//! than gated at build time, one [`HttpFeatureMode`] field each, defaulting off.
//!
//! [`ExtensionsConfig::cursor_pagination`] is the one extension that defaults
//! on. Its feature -- opaque, keyset-backed pagination pointers -- is what
//! clause 5.5.9.1 permits outright ("can be implemented in a completely opaque
//! way"), so a broker serving it is still a conformant broker; it is an
//! extension only in that the `cursor` parameter carrying the pointer is
//! Andromeda's own spelling, which an operator may want to switch off.
//!
//! [`ExtensionsConfig::conformance_mode`] is the hard override the ETSI
//! conformance runs set: when enabled it forces every extension off regardless
//! of the individual fields, so "the shipping binary passes the suite" is a
//! claim about the artifact operators run rather than about a special build.

use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// The built-in bound on how many temporal history instances one
/// delete-by-query may remove before it is refused with `TooManyResults`.
const DEFAULT_TEMPORAL_DELETE_MAX_MATCHES: u64 = 100_000;

/// The built-in bound on how many distinct aggregation groups one grouped
/// temporal aggregation may produce before it is refused with `TooManyResults`.
const DEFAULT_TEMPORAL_AGGREGATION_MAX_GROUPS: u64 = 10_000;

/// Invalid extensions configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ExtensionsConfigError {
    /// The temporal delete-by-query bound must admit at least one instance.
    ///
    /// A zero bound refuses every request the extension could serve, which is
    /// indistinguishable from disabling it but reports `TooManyResults` (403)
    /// where a disabled extension reports a plain 404/405. Disabling is the
    /// supported way to turn the extension off, so a zero bound is rejected.
    #[error("extensions.temporal_delete_by_query_max_matches must be at least 1")]
    ZeroTemporalDeleteMaxMatches,
    /// The temporal aggregation grouping bound must admit at least one group.
    ///
    /// A zero bound refuses every grouped aggregation with `TooManyResults`
    /// (403), which is indistinguishable from disabling the extension but for
    /// the error shape. Disabling is the supported way to turn it off, so a zero
    /// bound is rejected.
    #[error("extensions.temporal_aggregation_grouping_max_groups must be at least 1")]
    ZeroTemporalAggregationMaxGroups,
}

/// The `q_target_languages` setting: what a `q` bracket may name as a target
/// language, each level admitting everything the one before it does.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QTargetLanguages {
    /// The clause 4.9 ABNF alone: `[en]`, never `[en-GB]`.
    #[default]
    Specification,
    /// Every RFC 5646 language tag, matched by tag equality.
    LanguageTags,
    /// Every RFC 5646 tag and RFC 4647 extended range, matched by extended
    /// filtering.
    LanguageRanges,
}

/// Runtime configuration for the opt-in extensions.
///
/// Each feature is a [`HttpFeatureMode`] rather than a bare `bool`. Every one
/// but [`ExtensionsConfig::cursor_pagination`] defaults to
/// [`HttpFeatureMode::Disabled`], so a broker with no `[extensions]` section is
/// a conformant broker.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct ExtensionsConfig {
    /// The hard conformance override.
    ///
    /// When [`HttpFeatureMode::Enabled`] every extension is forced off no matter
    /// what the individual fields say, and the fact that it overrode them is
    /// logged loudly at startup. This is what CI sets when running the ETSI
    /// suite against the production binary.
    #[toml_comment(inline)]
    pub conformance_mode: HttpFeatureMode,
    /// Temporal delete-by-query on `DELETE /temporal/entities` (non-standard).
    ///
    /// Adds a windowed, selector-scoped temporal history purge that the NGSI-LD
    /// temporal-delete family (clauses 5.6.13, 5.6.15, 5.6.16) has no operation
    /// for. Local-scope only; never forwarded.
    #[toml_comment(inline)]
    pub temporal_delete_by_query: HttpFeatureMode,
    /// `datasetId` / `deleteAll` granularity on entity Purge (non-standard).
    ///
    /// Extends the standard Purge Entities `drop`/`keep` (clause 5.6.21) with
    /// the per-`datasetId` precision the single-entity Delete Attribute
    /// operation (clause 5.6.5) already has. When disabled, a `datasetId` or
    /// `deleteAll` on `DELETE /entities` is rejected with `InvalidRequest` and
    /// the standard Purge behaviour is unchanged.
    #[toml_comment(inline)]
    pub purge_dataset_scope: HttpFeatureMode,
    /// Grouped temporal aggregation via `aggrGroupBy` / `format=groupedValues`
    /// (non-standard).
    ///
    /// Adds a GROUP BY over the fixed `(entity, attribute, datasetId, period)`
    /// key that clause 4.5.19 never exposes: calendar and cyclic time buckets,
    /// per-entity-member keys, and value-in-effect keys, with mixed-unit groups
    /// (clause 4.5.2) split rather than refused. When disabled, `aggrGroupBy`
    /// and `groupedValues` are rejected with `InvalidRequest`.
    #[toml_comment(inline)]
    pub temporal_aggregation_grouping: HttpFeatureMode,
    /// Linked Entity retrieval on the temporal operations (non-standard).
    ///
    /// Clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4 refuse Linked Entity retrieval on
    /// Retrieve and Query Temporal Evolution and on the temporal Entity Map.
    /// When enabled, those operations accept `join`, `joinLevel` and
    /// `containedBy` and retrieve the Linked Entities' temporal evolution over
    /// the request's window by the clause 4.5.23 rules of the Core API. When
    /// disabled, the parameters are rejected with `InvalidRequest` on the
    /// temporal URL bindings, and a nested projection, a `q` hop or a
    /// retrieving `join` in a query body with `BadRequestData`.
    #[toml_comment(inline)]
    pub temporal_linked_retrieval: HttpFeatureMode,
    /// Tenant management on `/tenants` (non-standard).
    ///
    /// ETSI GS CIM 009 v1.9.1 leaves tenant management implementation-specific
    /// (clause 5.5.10). This routes the per-tenant introspection resource, the
    /// paginated tenant collection and suspend/resume via `PATCH isActive`, and
    /// advertises the surface at `/info/sourceIdentity`. When disabled, the
    /// `/tenants/{tenantName}` and `PATCH` routes are absent and nothing is
    /// advertised. No API operation ever drops a tenant's physical database.
    #[toml_comment(inline)]
    pub tenant_management: HttpFeatureMode,
    /// Opaque, keyset-backed cursor pagination (clause 5.5.9.1), on by default.
    ///
    /// Collections hand out `prev`/`next`/`last` pointers carrying an opaque
    /// `cursor` token and follow them back. Clause 5.5.9.1 permits pointers
    /// "implemented in a completely opaque way", which is why this is the one
    /// extension that defaults on. When disabled, `[pagination] default_mode`
    /// must be `offset`, and a `cursor` parameter is rejected with
    /// `InvalidRequest`.
    #[toml_comment(inline)]
    pub cursor_pagination: HttpFeatureMode,
    /// A `near` geo-query bounded from both sides (non-standard).
    ///
    /// The clause 4.10 `nearRel` grammar admits one of `maxDistance` and
    /// `minDistance`. When enabled, a `near` carrying both matches the
    /// entities at least `minDistance` and at most `maxDistance` away, on
    /// queries, temporal queries, Subscription `geoQ` and registration
    /// discovery. When disabled, the pair is refused with `BadRequestData`
    /// (clause 5.7.2.4).
    #[toml_comment(inline)]
    pub near_distance_band: HttpFeatureMode,
    /// `Accept-Language` read as the clause 4.15 language filter (non-standard).
    ///
    /// NGSI-LD carries the language filter in the `lang` parameter only. When
    /// enabled, a Retrieve or Query of Entities, Temporal Evolutions or Context
    /// Source Registrations that carries no `lang` takes its filter from the
    /// `Accept-Language` header, and each of those responses carries
    /// `Vary: Accept-Language` (RFC 9110 section 12.5.5). When disabled, the
    /// header is ignored, as RFC 9110 section 12.5.4 permits.
    #[toml_comment(inline)]
    pub accept_language: HttpFeatureMode,
    /// What a `q` bracket may name as a target language (non-standard above
    /// `specification`).
    ///
    /// The clause 4.9 ABNF admits only an `AttrName` in the bracket, so
    /// `specification` refuses `marque[en-GB]` with `BadRequestData`.
    /// `language-tags` admits every RFC 5646 tag, matched by case-insensitive
    /// tag equality (Class 1). `language-ranges` also admits RFC 4647
    /// extended ranges and matches every target language by section 3.3.2
    /// extended filtering, so `[en]` selects `en-GB` too (Class 2). A query
    /// carrying a target language is held to the level every Context Source
    /// it reaches advertises.
    #[toml_comment(inline)]
    pub q_target_languages: QTargetLanguages,
    /// The upper bound on the temporal history instances one delete-by-query may
    /// remove before it is refused with `TooManyResults` (403).
    ///
    /// Counted on a pre-count of the instances the operation would delete, so an
    /// oversized purge is refused whole rather than half-executed.
    pub temporal_delete_by_query_max_matches: u64,
    /// The upper bound on the distinct aggregation groups one grouped temporal
    /// aggregation may produce before it is refused with `TooManyResults` (403).
    ///
    /// Counted on a pre-count of the distinct groups the query would emit, so an
    /// oversized grouping is refused whole rather than partially returned.
    pub temporal_aggregation_grouping_max_groups: u64,
}

impl Default for ExtensionsConfig {
    fn default() -> ExtensionsConfig {
        ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Disabled,
            purge_dataset_scope: HttpFeatureMode::Disabled,
            temporal_aggregation_grouping: HttpFeatureMode::Disabled,
            temporal_linked_retrieval: HttpFeatureMode::Disabled,
            tenant_management: HttpFeatureMode::Disabled,
            cursor_pagination: HttpFeatureMode::Enabled,
            near_distance_band: HttpFeatureMode::Disabled,
            accept_language: HttpFeatureMode::Disabled,
            q_target_languages: QTargetLanguages::Specification,
            temporal_delete_by_query_max_matches: DEFAULT_TEMPORAL_DELETE_MAX_MATCHES,
            temporal_aggregation_grouping_max_groups: DEFAULT_TEMPORAL_AGGREGATION_MAX_GROUPS,
        }
    }
}

impl ExtensionsConfig {
    /// Validate the configured extension limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionsConfigError::ZeroTemporalDeleteMaxMatches`] when the
    /// temporal delete-by-query bound is zero, or
    /// [`ExtensionsConfigError::ZeroTemporalAggregationMaxGroups`] when the
    /// grouped-aggregation bound is zero.
    pub const fn validate(&self) -> Result<(), ExtensionsConfigError> {
        if self.temporal_delete_by_query_max_matches == 0 {
            return Err(ExtensionsConfigError::ZeroTemporalDeleteMaxMatches);
        }
        if self.temporal_aggregation_grouping_max_groups == 0 {
            return Err(ExtensionsConfigError::ZeroTemporalAggregationMaxGroups);
        }
        Ok(())
    }

    /// Whether the conformance override is forcing extensions off.
    #[must_use]
    pub const fn conformance_mode_enabled(&self) -> bool {
        matches!(self.conformance_mode, HttpFeatureMode::Enabled)
    }

    /// Whether any individual extension field asked to be on, before the
    /// conformance override is applied.
    ///
    /// This is what the startup log reports as "overridden": a field the
    /// operator turned on that `conformance_mode` then forced off.
    #[must_use]
    pub const fn any_feature_requested(&self) -> bool {
        matches!(self.temporal_delete_by_query, HttpFeatureMode::Enabled)
            || matches!(self.purge_dataset_scope, HttpFeatureMode::Enabled)
            || matches!(self.temporal_aggregation_grouping, HttpFeatureMode::Enabled)
            || matches!(self.temporal_linked_retrieval, HttpFeatureMode::Enabled)
            || matches!(self.tenant_management, HttpFeatureMode::Enabled)
            || matches!(self.cursor_pagination, HttpFeatureMode::Enabled)
            || matches!(self.near_distance_band, HttpFeatureMode::Enabled)
            || matches!(self.accept_language, HttpFeatureMode::Enabled)
            || !matches!(self.q_target_languages, QTargetLanguages::Specification)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{ExtensionsConfig, ExtensionsConfigError, QTargetLanguages};
    use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
    use toml_comment::TomlComment;

    #[test]
    fn every_extension_but_cursor_pagination_is_off_in_the_default_configuration() {
        let config = ExtensionsConfig::default();
        pretty_assertions::assert_eq!(config.conformance_mode, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.temporal_delete_by_query, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.purge_dataset_scope, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.temporal_aggregation_grouping, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.temporal_linked_retrieval, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.tenant_management, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.near_distance_band, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.accept_language, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.q_target_languages, QTargetLanguages::Specification);
    }

    /// Clause 5.5.9.1 permits opaque pointers outright, so cursor pagination is
    /// the one extension a default broker serves.
    #[test]
    fn cursor_pagination_is_enabled_by_default() {
        pretty_assertions::assert_eq!(ExtensionsConfig::default().cursor_pagination, HttpFeatureMode::Enabled);
    }

    #[test]
    fn a_zero_grouping_bound_is_rejected() {
        let config = ExtensionsConfig {
            temporal_aggregation_grouping_max_groups: 0,
            ..ExtensionsConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(ExtensionsConfigError::ZeroTemporalAggregationMaxGroups));
    }

    #[test]
    fn a_zero_delete_bound_is_rejected() {
        let config = ExtensionsConfig {
            temporal_delete_by_query_max_matches: 0,
            ..ExtensionsConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(ExtensionsConfigError::ZeroTemporalDeleteMaxMatches));
    }

    #[test]
    fn a_positive_delete_bound_validates() {
        assert!(ExtensionsConfig::default().validate().is_ok());
    }

    #[test]
    fn the_extension_fields_round_trip_through_toml() {
        let config = ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Enabled,
            purge_dataset_scope: HttpFeatureMode::Enabled,
            temporal_aggregation_grouping: HttpFeatureMode::Enabled,
            temporal_linked_retrieval: HttpFeatureMode::Enabled,
            tenant_management: HttpFeatureMode::Enabled,
            cursor_pagination: HttpFeatureMode::Disabled,
            near_distance_band: HttpFeatureMode::Enabled,
            accept_language: HttpFeatureMode::Enabled,
            q_target_languages: QTargetLanguages::LanguageTags,
            temporal_delete_by_query_max_matches: 42,
            temporal_aggregation_grouping_max_groups: 7,
        };
        let rendered = toml::to_string(&config).expect("config serialises to TOML");
        let parsed: ExtensionsConfig = toml::from_str(&rendered).expect("config parses from TOML");
        pretty_assertions::assert_eq!(parsed.temporal_delete_by_query, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.purge_dataset_scope, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.temporal_aggregation_grouping, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.temporal_linked_retrieval, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.cursor_pagination, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(parsed.near_distance_band, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.accept_language, HttpFeatureMode::Enabled);
        pretty_assertions::assert_eq!(parsed.q_target_languages, QTargetLanguages::LanguageTags);
        pretty_assertions::assert_eq!(parsed.temporal_delete_by_query_max_matches, 42);
        pretty_assertions::assert_eq!(parsed.temporal_aggregation_grouping_max_groups, 7);
    }

    #[test]
    fn the_extension_fields_appear_in_the_default_toml() {
        let rendered = ExtensionsConfig::default_toml();
        assert!(rendered.contains("conformance_mode"), "{rendered}");
        assert!(rendered.contains("temporal_delete_by_query"), "{rendered}");
        assert!(rendered.contains("purge_dataset_scope"), "{rendered}");
        assert!(rendered.contains("temporal_aggregation_grouping"), "{rendered}");
        assert!(rendered.contains("temporal_linked_retrieval"), "{rendered}");
        assert!(rendered.contains("cursor_pagination"), "{rendered}");
        assert!(rendered.contains("near_distance_band"), "{rendered}");
        assert!(rendered.contains("accept_language"), "{rendered}");
        assert!(rendered.contains("q_target_languages"), "{rendered}");
    }

    #[test]
    fn conformance_override_and_requested_features_are_read_off_the_fields() {
        let mut config = ExtensionsConfig {
            cursor_pagination: HttpFeatureMode::Disabled,
            ..ExtensionsConfig::default()
        };
        assert!(!config.conformance_mode_enabled());
        assert!(!config.any_feature_requested());
        config.temporal_delete_by_query = HttpFeatureMode::Enabled;
        assert!(config.any_feature_requested());
        config.conformance_mode = HttpFeatureMode::Enabled;
        assert!(config.conformance_mode_enabled());
    }

    /// The temporal Linked Entity retrieval extension is a requested feature
    /// like any other, so the conformance override reports it.
    #[test]
    fn temporal_linked_retrieval_counts_as_a_requested_feature() {
        let config = ExtensionsConfig {
            cursor_pagination: HttpFeatureMode::Disabled,
            temporal_linked_retrieval: HttpFeatureMode::Enabled,
            ..ExtensionsConfig::default()
        };
        assert!(config.any_feature_requested());
    }

    /// A level above the specification's own grammar is a requested feature,
    /// so the conformance override reports it.
    #[test]
    fn a_target_language_level_above_the_specification_counts_as_a_requested_feature() {
        for level in [QTargetLanguages::LanguageTags, QTargetLanguages::LanguageRanges] {
            let config = ExtensionsConfig {
                cursor_pagination: HttpFeatureMode::Disabled,
                q_target_languages: level,
                ..ExtensionsConfig::default()
            };
            assert!(config.any_feature_requested(), "{level:?}");
        }
    }

    #[test]
    fn the_target_language_levels_are_spelled_in_kebab_case() {
        for (level, token) in [
            (QTargetLanguages::Specification, "specification"),
            (QTargetLanguages::LanguageTags, "language-tags"),
            (QTargetLanguages::LanguageRanges, "language-ranges"),
        ] {
            let config = ExtensionsConfig {
                q_target_languages: level,
                ..ExtensionsConfig::default()
            };
            let rendered = toml::to_string(&config).expect("config serialises to TOML");
            assert!(rendered.contains(&format!("q_target_languages = \"{token}\"")), "{rendered}");
            let parsed: ExtensionsConfig = toml::from_str(&rendered).expect("config parses from TOML");
            pretty_assertions::assert_eq!(parsed.q_target_languages, level);
        }
    }

    /// The default-on cursor pagination is a requested feature, so conformance
    /// mode overriding it is reported like any other override.
    #[test]
    fn the_default_cursor_pagination_counts_as_a_requested_feature() {
        assert!(ExtensionsConfig::default().any_feature_requested());
    }
}
