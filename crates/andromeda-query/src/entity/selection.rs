//! Which entities an entity query matches.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExpr};
use andromeda_domain::uri::Uri;
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// The filters that decide which entities an entity query returns.
///
/// Everything here narrows the result set; nothing here affects how a matching
/// entity is rendered. That separation is what lets `build_ids_query` — which
/// only ever needs the identifiers — read this alone.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct EntitySelection {
    /// Resource identifiers to filter by. Empty means no identifier filter.
    pub ids: Vec<Uri>,
    /// Regular expression the entity id must match (clause 5.7.2).
    pub id_pattern: Option<Pattern>,
    /// Boolean expression over entity type IRIs (clause 4.17).
    ///
    /// `None` when no type filter is specified.
    pub type_expr: Option<TypeExpr>,
    /// Expanded attribute IRIs the entity must carry. Empty means no attribute
    /// filter.
    pub attrs: Vec<IriBuf>,
    /// Parsed `q` expression for property-level filtering (clause 4.9).
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified (clause 4.10).
    pub geo: Option<GeoQuery>,
    /// Scope query filtering by the entity's current scope hierarchy
    /// (clause 4.18).
    pub scope_q: Option<ScopeQuery>,
}
