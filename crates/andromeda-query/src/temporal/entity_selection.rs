//! Which entities a temporal query reads history for.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pattern::Pattern, scope::query::ScopeQuery};
use andromeda_domain::uri::Uri;
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// The entity-level filters of a temporal query.
///
/// Every field here narrows *which entities* qualify, and stage 1 of the
/// two-stage temporal statement is built from this alone. It is deliberately
/// separate from the time window and from the response shape: an entity either
/// qualifies or it does not, independently of which instants are returned for it
/// or how they are rendered.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct EntitySelection {
    /// Resource identifiers to filter by. Empty means no identifier filter.
    pub ids: Vec<Uri>,
    /// Regular expression the entity id must match (clause 5.7.2).
    pub id_pattern: Option<Pattern>,
    /// Entity type IRIs to filter by. Empty means no type filter.
    pub types: Vec<IriBuf>,
    /// Parsed `q` expression for property-level filtering (clause 4.9).
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified (clause 4.10).
    pub geo: Option<GeoQuery>,
    /// Scope query filtering by the entity's scope hierarchy (clause 4.18).
    ///
    /// Temporal queries match against `scope_history` rather than the entity's
    /// current scopes, bounded by the query's time window.
    pub scope_q: Option<ScopeQuery>,
}
