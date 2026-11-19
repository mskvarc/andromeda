//! The query expressions of a Subscription as a client reads them.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{attribute_name_csv, query::temporal_query::TemporalQuery};
use andromeda_ngsi_ld_query::{
    dataset_selector::DatasetSelector,
    filter::Filter,
    geo::geo_constraint::GeoConstraint,
    name_stage::Written,
    scope::scope_query::ScopeQuery,
};
use serde::Serialize;

/// The `q`, `geoQ`, `scopeQ`, `csf`, `datasetId`, `temporalQ`, `expandValues` and `jsonKeys` members (clauses 4.9, 4.10, 4.19 and 5.2.21), the names in the terms of the reader.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FiltersView<'a> {
    /// NGSI-LD query expression to filter matching entities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) q: Option<&'a Filter<Written>>,
    /// Geo-spatial query expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) geo_q: Option<&'a GeoConstraint<Written>>,
    /// Scope query expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) scope_q: Option<&'a ScopeQuery>,
    /// Context source filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) csf: Option<&'a Filter<Written>>,
    /// The `datasetId` selectors of the Attribute instances each notification carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) dataset_id: Option<&'a [DatasetSelector]>,
    /// Temporal query expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) temporal_q: Option<&'a TemporalQuery>,
    /// Attributes whose `q` values are expanded by JSON-LD type coercion.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "attribute_name_csv::serialize")]
    pub(super) expand_values: &'a Option<Vec<AttributeName>>,
    /// Attributes whose values are uninterpretable as JSON-LD.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "attribute_name_csv::serialize")]
    pub(super) json_keys: &'a Option<Vec<AttributeName>>,
}
