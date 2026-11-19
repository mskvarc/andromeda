//! The query expressions that narrow a subscription's matches.

use crate::{attribute_iri_csv, query::temporal_query::TemporalQuery};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::{
    dataset_selector::DatasetSelector,
    filter::Filter,
    geo::geo_constraint::GeoConstraint,
    name_stage::Expanded,
    scope::scope_query::ScopeQuery,
};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The query expressions that narrow a subscription's matches further
/// (ETSI GS CIM 009 clauses 4.9, 4.10, 4.19 and 5.2.21).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionFilters {
    /// NGSI-LD query expression to filter matching entities (clause 4.9).
    ///
    /// Held in the expanded stage: every name is the absolute IRI the request
    /// `@context` expanded it to (clause 5.5.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<Filter<Expanded>>,
    /// Geo-spatial query expression (clause 5.2.13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_q: Option<GeoConstraint<Expanded>>,
    /// Scope query expression (clause 4.19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<ScopeQuery>,
    /// Context source filter (clause 4.9), which is a `q` filter
    /// over a registration's own properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<Filter<Expanded>>,
    /// The `datasetId`s of the Attribute instances each notification carries,
    /// `@none` naming the default instance (Table 5.2.12-1, clause 4.5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Temporal query expression (clause 5.2.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<TemporalQuery>,
    /// Attributes whose `q` values are expanded through the subscription's
    /// `@context` by JSON-LD type coercion before the filter is evaluated
    /// (Table 5.2.12-1 `expandValues`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_iri_csv::deserialize",
        serialize_with = "attribute_iri_csv::serialize"
    )]
    pub expand_values: Option<Vec<AttributeIri>>,
    /// Attributes whose values are uninterpretable as JSON-LD, so only the
    /// object keys addressed by the `q` filter are expanded through the
    /// `@context` (Table 5.2.12-1 `jsonKeys`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_iri_csv::deserialize",
        serialize_with = "attribute_iri_csv::serialize"
    )]
    pub json_keys: Option<Vec<AttributeIri>>,
}
