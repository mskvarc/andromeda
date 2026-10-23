//! The compiled form of a Context Source Registration discovery query.

use crate::{
    filter::expression::Expr,
    geo::query::GeoQuery,
    pattern::Pattern,
    scope::query::ScopeQuery,
    temporal::window::TimeInterval,
    type_selection::grammar::TypeExpr,
};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier, time_prop::TimeProp};
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// A fully compiled Context Source Registration (CSR) discovery query
/// (clause 5.10.2).
///
/// The entity specification (`ids`, `entity_type`, `id_pattern`) and the
/// attribute list are matched against the registrations' `information` as
/// clause 5.12 describes; the context source filter, the geoquery, the scope
/// query and the temporal window are matched against the registrations' own
/// members (clause 5.10.2.4).
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct CompiledCsrQuery {
    /// Entity identifiers matched against `information[].entities[]` (clause
    /// 5.12). Empty means no identifier constraint.
    pub ids: Vec<Identifier>,
    /// The expanded clause 4.17 type selector matched against registered
    /// entity types (clause 5.12).
    pub entity_type: Option<TypeExpr>,
    /// Optional entity identifier pattern matched against registered entity
    /// identifiers and patterns (clause 5.12).
    pub id_pattern: Option<Pattern>,
    /// Expanded attribute IRIs to match against registered property or
    /// relationship names: the explicit `attrs` list together with the
    /// attributes the `q` filter names (clause 5.10.2.4). Empty means no
    /// attribute constraint.
    pub attrs: Vec<IriBuf>,
    /// The expanded context source filter, evaluated against the
    /// registrations' Context Source Properties (clause 4.9 `csf`).
    pub csf_expr: Option<Expr>,
    /// Geo-spatial filter against the registration's `location`.
    pub geo: Option<GeoQuery>,
    /// Scope query matched against the registration's `scope` (clause 4.19).
    pub scope_q: Option<ScopeQuery>,
    /// The context sources the request already passed through (clause 4.3.6.4).
    /// A registration whose `contextSourceAlias` is listed never matches
    /// (clause 5.12).
    pub encountered_sources: Vec<ContextSourcePseudonym>,
    /// Temporal window against the CSR observation or management interval.
    pub temporal: Option<CompiledCsrTemporalFilter>,
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
