//! The queries a Snapshot materialises (ETSI GS CIM 009 clause 5.2.23).
//!
//! A `Snapshot` stores its queries as raw JSON and parses them at execution
//! time, so these are the read side of `snapshotQueries` and
//! `snapshotTemporalQueries` rather than members of the resource itself.

use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::GeometryType};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    reference::NgsiLdReference,
    rfc3339,
    time_prop::TimeProp,
    time_rel::TimeRel,
    uri::Uri,
};
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    pattern::Pattern,
    scope::query::ScopeQuery,
    temporal::aggregation_period::{self, AggregationPeriod},
    type_selection::grammar::TypeExprRaw,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{DisplayFromStr, StringWithSeparator, formats::CommaSeparator, serde_as};
use typed_builder::TypedBuilder;

/// Parsed snapshot query (spec 5.2.23, Query type).
///
/// Used to deserialize elements of `snapshotQueries` and `snapshotTemporalQueries`.
/// Non-temporal queries have `temporal_q: None`; temporal queries include it.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    /// Entity selectors restricting the query by type, ID, or ID pattern.
    #[builder(default)]
    pub entities: Option<Vec<SnapshotEntitySelector>>,
    /// NGSI-LD query expression for property-level filtering (clause 4.9).
    #[builder(default)]
    pub q: Option<Expr>,
    /// Geo-spatial query filter.
    #[builder(default)]
    pub geo_q: Option<SnapshotGeoQuery>,
    /// Scope query, comma for OR and semicolon for AND (clause 4.18).
    #[builder(default)]
    pub scope_q: Option<ScopeQuery>,
    /// Attribute names to include in the result.
    #[builder(default)]
    pub attrs: Option<Vec<AttributeName>>,
    /// Attribute names to exclude from the result.
    #[builder(default)]
    pub omit: Option<Vec<AttributeName>>,
    /// Temporal query parameters (present only in `snapshotTemporalQueries`).
    #[builder(default)]
    pub temporal_q: Option<SnapshotTemporalQuery>,
}

/// Entity selector within a snapshot query (spec 5.2.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntitySelector {
    /// Specific entity IDs to match.
    pub id: Option<Vec<Uri>>,
    /// Regular expression the entity id must match (clause 4.5.6).
    pub id_pattern: Option<Pattern>,
    /// Type selection expression the entity's types must satisfy (clause 4.17).
    #[serde(rename = "type")]
    pub type_: Option<TypeExprRaw>,
}

/// Geo-spatial query within a snapshot query (spec 5.2.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotGeoQuery {
    /// Spatial relationship (clause 4.10).
    pub georel: GeoRel,
    /// GeoJSON geometry type (RFC 7946 section 1.4).
    pub geometry: GeometryType,
    /// GeoJSON coordinates.
    pub coordinates: Value,
    /// Name of the GeoProperty to query against (clause 4.10); `location` when
    /// absent.
    ///
    /// A reference rather than an IRI: the term reaches this member before
    /// JSON-LD expansion, so it may be a short name, a compact IRI or an
    /// expanded one, which is exactly the range clause 4.6.2 admits.
    pub geoproperty: Option<NgsiLdReference>,
}

/// Temporal query within a snapshot temporal query (spec 5.2.23).
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTemporalQuery {
    /// Temporal relationship the window is expressed with (clause 4.11).
    #[serde_as(as = "DisplayFromStr")]
    pub timerel: TimeRel,
    /// Reference time point.
    #[serde(with = "rfc3339::datetime")]
    pub time_at: DateTime<Utc>,
    /// End time point, required when `timerel` is `between`.
    #[serde(with = "rfc3339::optional_datetime", default)]
    #[builder(default)]
    pub end_time_at: Option<DateTime<Utc>>,
    /// Maximum number of temporal instances per attribute.
    #[builder(default)]
    pub last_n: Option<u32>,
    /// Temporal property the window applies to (clause 4.8); `observedAt` when absent.
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[builder(default)]
    pub timeproperty: Option<TimeProp>,
    /// Aggregation methods (clause 4.5.19), comma-separated on the wire.
    #[serde_as(as = "Option<StringWithSeparator::<CommaSeparator, AggrMethod>>")]
    #[builder(default)]
    pub aggr_methods: Option<Vec<AggrMethod>>,
    /// ISO 8601 duration for the aggregation bucket size (clause 4.5.19).
    ///
    /// Absent, empty and zero all mean one bucket over the whole range, so all
    /// three deserialize to `None`.
    #[serde(with = "aggregation_period::optional", default)]
    #[builder(default)]
    pub aggr_period_duration: Option<AggregationPeriod>,
}
