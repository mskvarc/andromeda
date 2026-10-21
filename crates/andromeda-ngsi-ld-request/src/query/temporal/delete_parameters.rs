//! Parameters of the temporal history delete-by-query (an Andromeda extension).
//!
//! `DELETE /temporal/entities` composes the entity selection of a temporal
//! query, the attribute `drop`/`keep` and `datasetId`/`deleteAll` scoping of the
//! per-entity temporal delete (clause 5.6.13), and a time window — a
//! combination the specification's temporal-delete family has no operation for.
//! This parameter set is deliberately a subset of
//! [`TemporalQueryParameters`](crate::query::temporal::TemporalQueryParameters):
//! it carries only what selects the instances to remove, and none of the
//! read-only shaping (`format`, `lastN`, `pick`/`omit`, `join`, pagination,
//! ordering) a delete cannot use. Unknown parameters are refused (clause 6.3.20
//! pattern), which is what keeps a mistyped read-only parameter from being
//! silently ignored.

use crate::query::{
    deserialization::{
        OptionalGeoQuery,
        deserialize_optional_attribute_names,
        deserialize_optional_dataset_selectors,
        deserialize_optional_filter_expr,
        deserialize_optional_pattern,
        deserialize_optional_uri_csv,
    },
    temporal::deserialization::{deserialize_option_timeproperty, deserialize_option_timerel},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::instant_serde, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    pattern::Pattern,
    scope::query::ScopeQuery,
    type_selection::grammar::TypeExprRaw,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Raw parameters of one temporal history delete-by-query.
///
/// All values are in their raw (compact) form before context expansion, exactly
/// as the temporal query parameters are.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalDeleteParameters {
    /// Entity identifiers to select, comma-separated (`id`).
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub id: Option<Vec<Identifier>>,
    /// Entity types to select, as a type-selection expression (clause 4.17 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// A regular expression the selected identifiers must match (`idPattern`).
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// The `q` filter expression (clause 4.9), parsed at the request boundary.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at URL extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
    /// The scope query expression (clause 4.18 `scopeQ`).
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// The context source filter (clause 5.7.4.3); parsed but never forwarded,
    /// because the delete is local-only.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub csf: Option<Expr>,

    /// How the window relates to the given instants (clause 4.11 `timerel`).
    #[serde(default, deserialize_with = "deserialize_option_timerel")]
    pub timerel: Option<TimeRel>,
    /// The instant the window is anchored at (`timeAt`).
    #[serde(rename = "timeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub time_at: Option<DateTime<Utc>>,
    /// The closing instant, required by `timerel=between` (`endTimeAt`).
    #[serde(rename = "endTimeAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The temporal property the window applies to (clause 4.8 `timeproperty`).
    #[serde(default, deserialize_with = "deserialize_option_timeproperty")]
    pub timeproperty: Option<TimeProp>,

    /// Attributes whose instances are removed (clause 5.6.21 `drop`).
    /// Mutually exclusive with `keep`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub drop: Option<Vec<AttributeName>>,
    /// Attributes whose instances are spared; every other attribute is removed
    /// (clause 5.6.21 `keep`). Mutually exclusive with `drop`.
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub keep: Option<Vec<AttributeName>>,

    /// Dataset instances to remove, comma-separated (`datasetId`, clause 4.5.5).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Whether every `datasetId` instance is removed (clause 5.6.13 `deleteAll`).
    /// Contradictory with `datasetId`.
    #[serde(rename = "deleteAll")]
    pub delete_all: Option<bool>,

    /// Whether the operation only reports the would-delete count and removes
    /// nothing (`dryRun`).
    #[serde(rename = "dryRun")]
    pub dry_run: Option<bool>,
}

impl TemporalDeleteParameters {
    /// Whether the request asked for a dry run.
    #[must_use]
    pub const fn is_dry_run(&self) -> bool {
        matches!(self.dry_run, Some(true))
    }

    /// Whether the request asked to remove every `datasetId` instance.
    #[must_use]
    pub const fn deletes_all_datasets(&self) -> bool {
        matches!(self.delete_all, Some(true))
    }

    /// The attribute names the `drop`/`keep` scope, whichever was given.
    #[must_use]
    pub fn scoped_attributes(&self) -> Option<&[AttributeName]> {
        self.drop.as_deref().or(self.keep.as_deref())
    }
}
