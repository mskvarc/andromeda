//! The compiled form of a Context Source Registration discovery query.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pagination::Pagination, pattern::Pattern, temporal::window::TimeInterval};
use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, uri::Uri};
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// A fully compiled Context Source Registration (CSR) query.
///
/// Used for querying `context_source_registrations` per NGSI-LD 5.10.2.
/// CSR queries support a subset of entity query features plus temporal filtering
/// against the registration's observation/management intervals.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, TypedBuilder)]
pub struct CompiledCsrTemporalFilter {
    /// The slice of the time axis the registration must cover.
    pub interval: TimeInterval,
    /// The time property to filter on.
    /// Determines whether the observation or management interval is checked.
    #[builder(default = Default::default())]
    pub timeproperty: Option<TimeProp>,
}
