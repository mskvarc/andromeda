//! Which entities a query matches.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExpr};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// One entity selector: an identifier list, an identifier pattern and a type
/// expression a candidate must satisfy together (clause 5.2.11).
///
/// A selector is the unit a POST query body's `entities[]` array is a union of.
/// `{id:A,type:T1}` and `{id:B,type:T2}` are two selectors, and an entity
/// matches the query when it satisfies *either* one, never the cross product
/// `(A,T2)` that neither named. A GET query, whose `id` and `type` parameters
/// are independent lists over one implicit group, is a single selector.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct EntitySelector {
    /// Resource identifiers this selector admits. Empty means no identifier
    /// constraint (clause 5.7.2 `id`).
    pub ids: Vec<Uri>,
    /// Regular expression the entity id must match (clause 5.7.2 `idPattern`).
    pub id_pattern: Option<Pattern>,
    /// Boolean expression over entity type IRIs (clause 4.17 `type`).
    ///
    /// `None` when this selector places no type constraint.
    pub type_expr: Option<TypeExpr>,
}

impl EntitySelector {
    /// Whether this selector constrains a candidate at all.
    ///
    /// An unconstrained selector — no id, no pattern, no type — admits every
    /// entity, so a union that contains one is unconstrained on these axes.
    pub const fn constrains(&self) -> bool {
        !self.ids.is_empty() || self.id_pattern.is_some() || self.type_expr.is_some()
    }
}

/// The filters that decide which entities a query returns.
///
/// Everything here narrows the result set; nothing here affects how a matching
/// entity is rendered, or which of its attribute instances come back. That
/// separation is what lets `build_ids_query` — which only ever needs the
/// identifiers — read this alone, and stage 1 of a temporal statement read it
/// without seeing the time window.
///
/// The identifier, pattern and type axes are grouped into [`EntitySelector`]s
/// and combined as a union (clause 5.2.11): an entity matches when it satisfies
/// any selector. The remaining members — `attrs`, `q`, `geo`, `scopeQ` — are
/// global, and hold for every selector at once. One type serves the entity and
/// temporal paths; a selection with no member set matches every entity, which
/// is what makes `Default` meaningful here: it is the empty filter, and a
/// caller describing the predicates a store *declined* starts from it.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct EntitySelection {
    /// The entity selectors, combined as a union (clause 5.2.11).
    ///
    /// Empty — or a single unconstrained selector — places no id/pattern/type
    /// constraint. A GET query holds one selector; a POST `entities[]` body
    /// holds one per array entry.
    pub selectors: Vec<EntitySelector>,
    /// Expanded attribute IRIs the entity must carry. Empty means no attribute
    /// filter.
    pub attrs: Vec<IriBuf>,
    /// Parsed `q` expression for property-level filtering (clause 4.9).
    pub q_expr: Option<Expr>,
    /// Geo-spatial query filter, if specified (clause 4.10).
    pub geo: Option<GeoQuery>,
    /// Scope query filtering by the entity's scope hierarchy (clause 4.18).
    ///
    /// A temporal query matches against `scope_history` rather than the
    /// entity's current scopes, bounded by the query's time window.
    pub scope_q: Option<ScopeQuery>,
}

impl EntitySelection {
    /// Whether the union of selectors constrains the id/pattern/type axes.
    ///
    /// True only when there is at least one selector and every selector
    /// constrains: an unconstrained selector admits every entity, so a union
    /// containing one places no constraint. Both the SQL renderer and the
    /// in-memory residual read this, so the pushed-down and finished-in-memory
    /// readings of a union cannot disagree.
    pub fn selectors_constrain(&self) -> bool {
        !self.selectors.is_empty() && self.selectors.iter().all(EntitySelector::constrains)
    }
}
