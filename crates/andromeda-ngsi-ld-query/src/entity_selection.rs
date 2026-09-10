//! Which entities a query matches.

use crate::{filter::expression::Expr, geo::query::GeoQuery, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExpr};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// One entity selector: an identifier list, an identifier pattern and a type
/// expression a candidate must satisfy together (clause 5.2.33).
///
/// A selector is the unit a POST query body's `entities[]` array is a union of.
/// `{id:A,type:T1}` and `{id:B,type:T2}` are two selectors, and an entity
/// matches the query when it satisfies *either* one, never the cross product
/// `(A,T2)` that neither named. A GET query, whose `id` and `type` parameters
/// are independent lists over one implicit group, is a single selector.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
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
    #[must_use]
    pub const fn constrains(&self) -> bool {
        !self.ids.is_empty() || self.id_pattern.is_some() || self.type_expr.is_some()
    }
}

/// Which entities a request is allowed to consider at all, before any selector
/// or filter runs (clause 5.5.14).
///
/// A list of identifiers cannot express this on its own: an empty
/// [`EntitySelector::ids`] reads as "no identifier constraint", so it can only
/// stand for a request that named no identifier, never for one that named an
/// empty set of them. Clause 5.5.14 makes the second case routine — "An
/// `EntityMap` fixes the Entities to be considered for subsequent requests based
/// on it", and a request scoped to a reused `EntityMap` is left with an empty
/// set whenever the map lists none of the identifiers it asks for, or has been
/// emptied by the clause 4.3.6.7 pruning. Such a request considers no entity,
/// and answering it with everything the remaining filters match would widen the
/// very answer the map exists to fix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConsideredEntities {
    /// Every entity the selectors and filters admit: the request named an
    /// identifier set, or none at all, that still leaves entities to consider.
    #[default]
    Selected,
    /// No entity at all: the request named an empty set of identifiers, which
    /// nothing can satisfy.
    NoEntity,
}

impl ConsideredEntities {
    /// What a request's identifier parameter leaves to consider.
    ///
    /// `None` is a request that named no identifier and so fixes nothing;
    /// `Some` of an empty slice is a request that named an empty set of them,
    /// which is what a query scoped to a reused `EntityMap` is narrowed to when
    /// nothing survives the intersection (clause 5.5.14).
    #[must_use]
    pub const fn of_request(ids: Option<&[Uri]>) -> ConsideredEntities {
        match ids {
            Some([]) => ConsideredEntities::NoEntity,
            Some([_, ..]) | None => ConsideredEntities::Selected,
        }
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
/// and combined as a union (clause 5.2.33): an entity matches when it satisfies
/// any selector. The remaining members — `attrs`, `q`, `geo`, `scopeQ` and
/// `considered` — are global, and hold for every selector at once. One type
/// serves the entity and temporal paths; a selection with no member set matches
/// every entity, which is what makes `Default` meaningful here: it is the empty
/// filter, and a caller describing the predicates a store *declined* starts
/// from it.
#[derive(Debug, Clone, Default, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct EntitySelection {
    /// The entity selectors, combined as a union (clause 5.2.33).
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
    /// Which entities the request may consider at all (clause 5.5.14).
    ///
    /// Conjoined with everything else: [`ConsideredEntities::NoEntity`] admits
    /// nothing whatever the selectors match, which is how the empty
    /// intersection of a request and a reused `EntityMap` reaches the store as
    /// a predicate rather than dissolving into "no identifier constraint".
    pub considered: ConsideredEntities,
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

    /// Drop every identifier constraint, leaving the pattern, type and value
    /// filters to decide alone.
    ///
    /// Clause 5.5.14 lets each request under an `EntityMap` specify its own
    /// entity identifiers while holding everything else to the request that
    /// created the map, so the identifiers are not among the filters clause
    /// 4.3.6.7 rechecks before an entity joins a result page. A recheck runs
    /// the selection this leaves.
    pub fn without_identifier_narrowing(&mut self) {
        for selector in &mut self.selectors {
            selector.ids.clear();
        }
        self.considered = ConsideredEntities::Selected;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_selection::{ConsideredEntities, EntitySelection, EntitySelector},
        pattern::Pattern,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;

    /// Clause 5.5.14: a request that named no identifier fixes nothing, so the
    /// selectors and filters alone decide what it considers.
    #[test]
    fn a_request_naming_no_identifier_considers_whatever_it_selects() {
        pretty_assertions::assert_eq!(ConsideredEntities::of_request(None), ConsideredEntities::Selected);
    }

    /// Clause 5.5.14: a request narrowed to an empty identifier set — a query
    /// scoped to a reused `EntityMap` that lists none of the identifiers it
    /// asks for — considers no entity at all.
    #[test]
    fn a_request_naming_an_empty_identifier_set_considers_no_entity() {
        pretty_assertions::assert_eq!(ConsideredEntities::of_request(Some(&[])), ConsideredEntities::NoEntity);
    }

    /// A request that named identifiers considers them, whatever their number.
    #[test]
    fn a_request_naming_identifiers_considers_whatever_it_selects() {
        let ids: [Uri; 1] = ["urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")];
        pretty_assertions::assert_eq!(ConsideredEntities::of_request(Some(&ids)), ConsideredEntities::Selected);
    }

    /// The empty filter considers every selected entity, so a caller that
    /// describes only the predicates a store declined never narrows by accident.
    #[test]
    fn the_default_selection_considers_whatever_it_selects() {
        pretty_assertions::assert_eq!(EntitySelection::default().considered, ConsideredEntities::Selected);
    }

    /// Clause 5.5.14: the identifiers a request specifies are the one parameter
    /// a follow-up under an `EntityMap` may change, so dropping them leaves the
    /// filters the clause 4.3.6.7 recheck applies — including the narrowing to
    /// no entity, which is itself an identifier constraint.
    #[test]
    fn dropping_the_identifier_narrowing_leaves_the_other_filters_alone() {
        let pattern = Pattern::parse("^urn:").expect("valid pattern");
        let mut selection = EntitySelection::builder()
            .selectors(vec![
                EntitySelector::builder()
                    .ids(vec!["urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")])
                    .id_pattern(Some(pattern))
                    .build(),
            ])
            .considered(ConsideredEntities::NoEntity)
            .build();
        selection.without_identifier_narrowing();
        pretty_assertions::assert_eq!(selection.selectors[0].ids, Vec::<Uri>::new());
        assert!(selection.selectors[0].id_pattern.is_some(), "only the identifiers are dropped");
        pretty_assertions::assert_eq!(selection.considered, ConsideredEntities::Selected);
    }
}
