//! The compiled form of a Context Source Registration discovery query.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pagination::Pagination, pattern::Pattern};
use andromeda_domain::{
    temporal::{TimeProp, TimeRel},
    uri::Uri,
};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// A fully compiled Context Source Registration (CSR) query.
///
/// Used for querying `context_source_registrations` per NGSI-LD 5.10.2.
/// CSR queries support a subset of entity query features plus temporal filtering
/// against the registration's observation/management intervals.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct CompiledCsrQuery {
    /// Resource identifiers to filter by. Empty means no identifier filter.
    pub ids: Vec<Uri>,
    /// Optional expanded entity type IRI to match against registered entity types.
    pub entity_type: Option<IriBuf>,
    /// Optional ID pattern to match against registered entity ID patterns.
    pub id_pattern: Option<Pattern>,
    /// Expanded attribute IRIs to match against registered property or
    /// relationship names. Empty means no attribute filter.
    pub attrs: Vec<IriBuf>,
    /// Parsed `q` expression for filtering against CSR `additional_properties`.
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter against the CSR `location`.
    pub geo: Option<GeoQuery>,
    /// Temporal filter against the CSR observation or management interval.
    pub temporal: Option<CompiledCsrTemporalFilter>,
    /// Result window: limit, offset (count ignored for CSR queries).
    #[builder(default = Pagination::DEFAULT)]
    pub pagination: Pagination,
}

/// Temporal filter component of a CSR query.
///
/// Filters Context Source Registrations based on their temporal coverage,
/// matching against the `observationInterval` or `managementInterval` fields.
#[derive(Debug, Clone, PartialEq, Eq, TypedBuilder)]
pub struct CompiledCsrTemporalFilter {
    /// Temporal relationship operator.
    pub timerel: TimeRel,
    /// The reference time point.
    pub time_at: DateTime<Utc>,
    /// The end time point for `between` queries.
    #[builder(default)]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The time property to filter on.
    /// Determines whether the observation or management interval is checked.
    #[builder(default)]
    pub timeproperty: Option<TimeProp>,
}
