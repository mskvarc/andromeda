//! The `[extensions]` tables: Andromeda's opt-in, non-standard features.
//!
//! Every extension is additive: inert unless a client asks for it, and its
//! "off" state is exactly the behaviour of a conformant broker that never
//! implemented it. Each has a table of its own with a `mode`, off by default
//! but for cursor pagination, whose opaque pointers clause 5.5.9.1 permits
//! outright.
//!
//! `[extensions] mode = "conformance"` is the hard override the ETSI
//! conformance runs set: it forces every extension off whatever its own table
//! says, so "the shipping binary passes the suite" is a claim about the
//! artifact operators run rather than about a special build.

pub mod bounded;
pub mod cursor_pagination;
pub mod switch;

use crate::schema::{
    extensions::{
        bounded::{TemporalAggregationGroupingConfig, TemporalDeleteByQueryConfig},
        cursor_pagination::CursorPaginationConfig,
        switch::ExtensionSwitch,
    },
    feature_mode::FeatureMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether the extensions follow their own tables or are all forced off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether the extensions follow their own tables or are all forced off.")]
pub enum ExtensionsMode {
    /// Each extension is served as its own table says.
    #[default]
    #[schemars(description = "Each extension is served as its own table says.")]
    Configured,
    /// Every extension is off, cursor pagination included, whatever its own
    /// table says; the override is logged at startup.
    #[schemars(
        description = "Every extension is off, cursor pagination included, whatever its own table says; each override is logged at startup. The ETSI conformance runs set it, so the shipped binary is the one tested."
    )]
    Conformance,
}

/// What a `q` bracket may name as a target language, each level admitting
/// everything the one before it does: what a broker admits, as
/// `[extensions.q_target_languages]` names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Display)]
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

/// What a `q` bracket may name beyond the specification's grammar while the
/// extension is on, each level admitting everything the one before it does.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default, PartialEq, Eq, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "What a `q` bracket may name as a target language while the extension is on, each level admitting everything the one before it does.")]
pub enum QTargetLanguageLevel {
    /// Every RFC 5646 language tag, matched by tag equality.
    #[default]
    #[schemars(description = "Every RFC 5646 language tag, matched by case-insensitive tag equality.")]
    LanguageTags,
    /// Every RFC 5646 tag and RFC 4647 extended range, matched by extended
    /// filtering.
    #[schemars(
        description = "Every RFC 5646 tag and RFC 4647 extended range, matched by extended filtering, so `[en]` selects `en-GB` too. This changes what some queries the specification's grammar admits select."
    )]
    LanguageRanges,
}

/// The `[extensions.q_target_languages]` table.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "What a `q` bracket may name as a target language.")]
pub struct QTargetLanguagesConfig {
    /// Whether a `q` bracket may name more than the specification's grammar.
    #[schemars(
        description = "Whether a `q` bracket may name a language tag. Off, a bracket names only what the NGSI-LD `q` grammar defines: `[en]` is admitted, `[en-GB]` is refused with `BadRequestData`. A query carrying a target language is held to the level every context source it reaches advertises."
    )]
    pub mode: FeatureMode,
    /// What a bracket may name while the extension is on.
    #[schemars(description = "What a `q` bracket may name while the extension is on. Read only when `extensions.q_target_languages.mode` is `\"enabled\"`.")]
    pub level: QTargetLanguageLevel,
}

impl Default for QTargetLanguagesConfig {
    fn default() -> QTargetLanguagesConfig {
        QTargetLanguagesConfig {
            mode: FeatureMode::Disabled,
            level: QTargetLanguageLevel::default(),
        }
    }
}

impl From<&QTargetLanguagesConfig> for QTargetLanguages {
    fn from(config: &QTargetLanguagesConfig) -> QTargetLanguages {
        match (config.mode, config.level) {
            (FeatureMode::Disabled, QTargetLanguageLevel::LanguageTags | QTargetLanguageLevel::LanguageRanges) => QTargetLanguages::Specification,
            (FeatureMode::Enabled, QTargetLanguageLevel::LanguageTags) => QTargetLanguages::LanguageTags,
            (FeatureMode::Enabled, QTargetLanguageLevel::LanguageRanges) => QTargetLanguages::LanguageRanges,
        }
    }
}

impl From<QTargetLanguages> for QTargetLanguagesConfig {
    fn from(admitted: QTargetLanguages) -> QTargetLanguagesConfig {
        match admitted {
            QTargetLanguages::Specification => QTargetLanguagesConfig::default(),
            QTargetLanguages::LanguageTags => QTargetLanguagesConfig {
                mode: FeatureMode::Enabled,
                level: QTargetLanguageLevel::LanguageTags,
            },
            QTargetLanguages::LanguageRanges => QTargetLanguagesConfig {
                mode: FeatureMode::Enabled,
                level: QTargetLanguageLevel::LanguageRanges,
            },
        }
    }
}

/// Andromeda's opt-in, non-standard features.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(
    description = "Andromeda's opt-in, non-standard features, one table each. Every extension is inert unless a client asks for it, and its off state is the behaviour of a conformant broker."
)]
pub struct ExtensionsConfig {
    /// Whether the extensions follow their own tables (`configured`) or are
    /// all forced off (`conformance`), as the ETSI conformance runs set.
    #[schemars(description = "Whether the extensions follow their own tables (`\"configured\"`) or are all forced off (`\"conformance\"`).")]
    pub mode: ExtensionsMode,
    /// Opaque, keyset-backed cursor pagination (clause 5.5.9.1), the default
    /// paging of a fresh request unless set otherwise.
    #[schemars(description = "Opaque cursor page pointers, and how a request that names neither `offset` nor `cursor` is paged.")]
    pub cursor_pagination: CursorPaginationConfig,
    /// Temporal delete-by-query on `DELETE /temporal/entities`.
    ///
    /// A windowed, selector-scoped temporal history purge the NGSI-LD
    /// temporal-delete family (clauses 5.6.13, 5.6.15, 5.6.16) has no
    /// operation for. Local-scope only; never forwarded.
    #[schemars(
        description = "Temporal delete-by-query on `DELETE /temporal/entities`: a windowed, selector-scoped history purge the NGSI-LD temporal delete operations have no equivalent for."
    )]
    pub temporal_delete_by_query: TemporalDeleteByQueryConfig,
    /// Grouped temporal aggregation via `aggrGroupBy` and
    /// `format=groupedValues`.
    ///
    /// A GROUP BY over the fixed `(entity, attribute, datasetId, period)` key
    /// that clause 4.5.19 never exposes: calendar and cyclic time buckets,
    /// per-entity-member keys, and value-in-effect keys, with mixed-unit
    /// groups (clause 4.5.2) split rather than refused. Off, `aggrGroupBy` and
    /// `groupedValues` are refused with `InvalidRequest`.
    #[schemars(description = "Grouped temporal aggregation through `aggrGroupBy` and `format=groupedValues`.")]
    pub temporal_aggregation_grouping: TemporalAggregationGroupingConfig,
    /// Linked Entity retrieval on the temporal operations.
    ///
    /// Clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4 refuse Linked Entity retrieval
    /// on Retrieve and Query Temporal Evolution and on the temporal Entity
    /// Map. On, those operations accept `join`, `joinLevel` and `containedBy`
    /// and retrieve the Linked Entities' temporal evolution over the
    /// request's window by the clause 4.5.23 rules of the Core API. Off, the
    /// parameters are refused with `InvalidRequest` on the temporal URL
    /// bindings, and a nested projection, a `q` hop or a retrieving `join` in
    /// a query body with `BadRequestData`.
    #[schemars(
        description = "Linked entity retrieval on the temporal operations. On, Retrieve and Query Temporal Evolution and the temporal Entity Map accept `join`, `joinLevel` and `containedBy` and retrieve the linked entities' temporal evolution over the request's window. Off, those parameters are refused with `InvalidRequest`, and a nested projection, a `q` hop or a retrieving `join` in a query body with `BadRequestData`."
    )]
    pub temporal_linked_retrieval: ExtensionSwitch,
    /// `datasetId` and `deleteAll` granularity on entity Purge.
    ///
    /// Extends the standard Purge Entities `drop`/`keep` (clause 5.6.21) with
    /// the per-`datasetId` precision the single-entity Delete Attribute
    /// operation (clause 5.6.5) already has. Off, a `datasetId` or `deleteAll`
    /// on `DELETE /entities` is refused with `InvalidRequest`.
    #[schemars(
        description = "`datasetId` and `deleteAll` on Purge Entities. On, `DELETE /entities` gains the per-`datasetId` precision Delete Attribute already has. Off, a `datasetId` or `deleteAll` there is refused with `InvalidRequest`."
    )]
    pub purge_dataset_scope: ExtensionSwitch,
    /// Tenant management on `/tenants`.
    ///
    /// ETSI GS CIM 009 v1.9.1 leaves tenant management implementation-specific
    /// (clause 5.5.10). This routes the per-tenant introspection resource, the
    /// paginated tenant collection and suspend/resume via `PATCH isActive`,
    /// and advertises the surface at `/info/sourceIdentity`. No API operation
    /// ever drops a tenant's physical database.
    #[schemars(
        description = "Tenant management on `/tenants`. On, the broker serves the tenant collection, per-tenant introspection and suspend or resume through `PATCH isActive`, and advertises them at `/info/sourceIdentity`. No API operation ever drops a tenant's database."
    )]
    pub tenant_management: ExtensionSwitch,
    /// A `near` geo-query bounded from both sides.
    ///
    /// The clause 4.10 `nearRel` grammar admits one of `maxDistance` and
    /// `minDistance`. On, a `near` carrying both matches the entities at least
    /// `minDistance` and at most `maxDistance` away, on queries, temporal
    /// queries, Subscription `geoQ` and registration discovery. Off, the pair
    /// is refused with `BadRequestData` (clause 5.7.2.4).
    #[schemars(
        description = "A `near` geo-query bounded from both sides. On, a `near` carrying both `minDistance` and `maxDistance` matches the entities between the two, on queries, temporal queries, subscription `geoQ` and registration discovery. Off, the pair is refused with `BadRequestData`."
    )]
    pub near_distance_band: ExtensionSwitch,
    /// `Accept-Language` read as the clause 4.15 language filter.
    ///
    /// NGSI-LD carries the language filter in the `lang` parameter only. On,
    /// a Retrieve or Query of Entities, Temporal Evolutions or Context Source
    /// Registrations that carries no `lang` takes its filter from the
    /// `Accept-Language` field, and each of those responses carries
    /// `Vary: Accept-Language` (RFC 9110 section 12.5.5). Off, the field is
    /// ignored, as RFC 9110 section 12.5.4 permits.
    #[schemars(
        description = "`Accept-Language` read as the language filter. On, a Retrieve or Query of entities, Temporal Evolutions or context source registrations that carries no `lang` takes its filter from `Accept-Language`, and the response carries `Vary: Accept-Language`. Off, the header is ignored."
    )]
    pub accept_language: ExtensionSwitch,
    /// What a `q` bracket may name as a target language.
    ///
    /// The clause 4.9 ABNF admits only an `AttrName` in the bracket, so
    /// `specification` refuses `marque[en-GB]` with `BadRequestData`.
    /// `language-tags` admits every RFC 5646 tag, matched by case-insensitive
    /// tag equality (Class 1). `language-ranges` also admits RFC 4647
    /// extended ranges and matches every target language by section 3.3.2
    /// extended filtering, so `[en]` selects `en-GB` too (Class 2). A query
    /// carrying a target language is held to the level every Context Source
    /// it reaches advertises.
    #[schemars(description = "What a `q` bracket may name as a target language.")]
    pub q_target_languages: QTargetLanguagesConfig,
}

impl ExtensionsConfig {
    /// Whether the conformance override is forcing extensions off.
    #[must_use]
    pub const fn is_conformance(&self) -> bool {
        matches!(self.mode, ExtensionsMode::Conformance)
    }

    /// Whether any extension's own table asked for it, before the conformance
    /// override is applied: what the startup log reports as overridden.
    #[must_use]
    pub const fn any_feature_requested(&self) -> bool {
        self.temporal_delete_by_query.mode.is_enabled()
            || self.temporal_aggregation_grouping.mode.is_enabled()
            || self.temporal_linked_retrieval.is_requested()
            || self.purge_dataset_scope.is_requested()
            || self.tenant_management.is_requested()
            || self.near_distance_band.is_requested()
            || self.accept_language.is_requested()
            || self.cursor_pagination.mode.is_enabled()
            || self.q_target_languages.mode.is_enabled()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        extensions::{
            ExtensionsConfig,
            ExtensionsMode,
            QTargetLanguageLevel,
            QTargetLanguages,
            QTargetLanguagesConfig,
            cursor_pagination::{CursorPaginationConfig, CursorPaginationMode},
            switch::ExtensionSwitch,
        },
        feature_mode::FeatureMode,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn every_extension_but_cursor_pagination_is_off_by_default() {
        let config = ExtensionsConfig::default();
        assert_eq!(config.mode, ExtensionsMode::Configured);
        assert_eq!(CursorPaginationMode::from(&config.cursor_pagination), CursorPaginationMode::Default);
        assert_eq!(config.temporal_delete_by_query.mode, FeatureMode::Disabled);
        assert_eq!(config.temporal_aggregation_grouping.mode, FeatureMode::Disabled);
        for switch in [
            config.temporal_linked_retrieval,
            config.purge_dataset_scope,
            config.tenant_management,
            config.near_distance_band,
            config.accept_language,
        ] {
            assert_eq!(switch, ExtensionSwitch::default());
            assert!(!switch.is_requested());
        }
        assert_eq!(QTargetLanguages::from(&config.q_target_languages), QTargetLanguages::Specification);
    }

    #[test]
    fn every_extension_is_its_own_table_with_a_mode() {
        let rendered = toml::to_string(&ExtensionsConfig::default()).expect("the defaults render");
        for table in [
            "[cursor_pagination]",
            "[temporal_delete_by_query]",
            "[temporal_aggregation_grouping]",
            "[temporal_linked_retrieval]",
            "[purge_dataset_scope]",
            "[tenant_management]",
            "[near_distance_band]",
            "[accept_language]",
            "[q_target_languages]",
        ] {
            assert!(rendered.contains(&format!("{table}\nmode = ")), "{table} in:\n{rendered}");
        }
        assert_eq!(
            toml::from_str::<ExtensionsConfig>(&rendered).expect("the defaults read back"),
            ExtensionsConfig::default()
        );
    }

    #[test]
    fn the_target_language_levels_are_spelled_in_kebab_case() {
        for (level, token) in [
            (QTargetLanguageLevel::LanguageTags, "language-tags"),
            (QTargetLanguageLevel::LanguageRanges, "language-ranges"),
        ] {
            let parsed: QTargetLanguagesConfig = toml::from_str(&format!("mode = \"enabled\"\nlevel = \"{token}\"\n")).expect("a level");
            assert_eq!(parsed.level, level);
        }
    }

    #[test]
    fn what_a_broker_admits_in_a_q_bracket_is_named_by_one_table() {
        for admitted in [
            QTargetLanguages::Specification,
            QTargetLanguages::LanguageTags,
            QTargetLanguages::LanguageRanges,
        ] {
            assert_eq!(QTargetLanguages::from(&QTargetLanguagesConfig::from(admitted)), admitted);
        }
        let disabled: QTargetLanguagesConfig = toml::from_str("mode = \"disabled\"\nlevel = \"language-ranges\"\n").expect("a table");
        assert_eq!(QTargetLanguages::from(&disabled), QTargetLanguages::Specification);
        assert!(toml::from_str::<QTargetLanguagesConfig>("mode = \"language-tags\"\nlevel = \"language-tags\"\n").is_err());
    }

    #[test]
    fn the_extensions_mode_reads_configured_and_conformance_only() {
        let read = |token: &str| toml::from_str::<ExtensionsConfig>(&format!("mode = \"{token}\"\n")).map(|config| config.mode);
        assert!(read("enabled").is_err());
        let mut config = ExtensionsConfig::default();
        assert!(!config.is_conformance());
        config.mode = ExtensionsMode::Conformance;
        assert!(config.is_conformance());
    }

    /// The default-on cursor pagination and a target-language level above the
    /// specification are requested features, so the conformance override
    /// reports overriding them.
    #[test]
    fn requested_features_are_read_off_every_table() {
        let mut config = ExtensionsConfig {
            cursor_pagination: CursorPaginationConfig::from(CursorPaginationMode::Disabled),
            ..ExtensionsConfig::default()
        };
        assert!(!config.any_feature_requested());
        config.tenant_management = ExtensionSwitch::new(FeatureMode::Enabled);
        assert!(config.any_feature_requested());
        assert!(ExtensionsConfig::default().any_feature_requested());
        let languages = ExtensionsConfig {
            cursor_pagination: config.cursor_pagination,
            q_target_languages: QTargetLanguagesConfig::from(QTargetLanguages::LanguageRanges),
            ..ExtensionsConfig::default()
        };
        assert!(languages.any_feature_requested());
    }
}
