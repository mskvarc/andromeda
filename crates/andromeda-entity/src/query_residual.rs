//! The predicates a store declined to apply.
//!
//! A newtype over `EntitySelection` rather than a second eight-field struct:
//! the residual is drawn from exactly the same vocabulary of predicates, and
//! restating them would be two definitions of one concept. The wrapper exists
//! so a call site cannot confuse what was asked for with what is left over.
//!
//! The `matches()` evaluator that finishes the residual in memory over a
//! stored entity state lives in `crate::residual_filter`.

use crate::residual_predicate::ResidualPredicate;
use andromeda_ngsi_ld_query::{
    entity_selection::{ConsideredEntities, EntitySelection},
    geo::default_location_narrowing::DefaultLocationNarrowing,
    projection::projected_members::ProjectedMembers,
};

/// The subset of an [`EntitySelection`] a store did not apply.
///
/// Construction and structural inspection live here; the in-memory
/// evaluation is `crate::residual_filter::matches`.
#[derive(Debug, Default)]
pub struct ResidualFilter(EntitySelection);

impl ResidualFilter {
    /// A residual over the predicates in `selection`.
    #[must_use]
    pub const fn over(selection: EntitySelection) -> ResidualFilter {
        ResidualFilter(selection)
    }

    /// Borrow the underlying selection for field-level access.
    #[must_use]
    pub const fn selection(&self) -> &EntitySelection {
        &self.0
    }

    /// Whether the store applied every predicate, leaving nothing to finish.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kinds().is_empty()
    }

    /// Remove from `selection` every predicate this residual carries.
    ///
    /// What is left is the query a store actually issues: asking it to apply a
    /// predicate it has already said it cannot would either fail or, worse,
    /// silently render something weaker.
    pub fn strip_from(&self, selection: &mut EntitySelection) {
        // How `q_expr` reads its target languages is no predicate of its own,
        // and goes with `q_expr`.
        let EntitySelection {
            selectors,
            attrs,
            q_expr,
            language_matching: _,
            geo,
            default_location: _,
            scope_q,
            considered,
            projected_members,
        } = &self.0;
        // The selector axes are the same across every selector, so a residual
        // that carries one clears that axis from every selector of the target.
        // In production every store pushes id/idPattern/type down, so these
        // branches are exercised only by the conformance harness.
        if selectors.iter().any(|selector| !selector.ids.is_empty()) {
            selection.selectors.iter_mut().for_each(|selector| selector.ids.clear());
        }
        // Clause 5.5.14: what a reused EntityMap leaves to consider is an
        // identifier predicate, so a store that hands the identifier axis back
        // hands this back with it and the query it issues stops carrying it.
        match considered {
            ConsideredEntities::NoEntity => selection.considered = ConsideredEntities::Selected,
            ConsideredEntities::Selected => {}
        }
        if selectors.iter().any(|selector| selector.id_pattern.is_some()) {
            selection.selectors.iter_mut().for_each(|selector| selector.id_pattern = None);
        }
        if selectors.iter().any(|selector| selector.type_expr.is_some()) {
            selection.selectors.iter_mut().for_each(|selector| selector.type_expr = None);
        }
        if !attrs.is_empty() {
            selection.attrs.clear();
        }
        if q_expr.is_some() {
            selection.q_expr = None;
        }
        if geo.is_some() {
            // What an index established about the default `location` instances
            // narrows only the geo-query, and goes with it.
            selection.geo = None;
            selection.default_location = DefaultLocationNarrowing::Unnarrowed;
        }
        if scope_q.is_some() {
            selection.scope_q = None;
        }
        if projected_members.constrains() {
            selection.projected_members = ProjectedMembers::Unconditional;
        }
    }

    /// Which kinds of predicate are outstanding, in the order they are
    /// evaluated.
    #[must_use]
    pub fn kinds(&self) -> Vec<ResidualPredicate> {
        // An index's narrowing of the default `location` instances is part of
        // the geo-query, not a predicate of its own, and how `q_expr` reads its
        // target languages is part of `q_expr`.
        let EntitySelection {
            selectors,
            attrs,
            q_expr,
            language_matching: _,
            geo,
            default_location: _,
            scope_q,
            considered,
            projected_members,
        } = &self.0;
        let mut kinds = Vec::new();
        // Clause 5.5.14: considering no entity is a predicate over identifiers,
        // outstanding for whoever finishes the query in memory just as an
        // identifier list is.
        if selectors.iter().any(|selector| !selector.ids.is_empty()) || matches!(considered, ConsideredEntities::NoEntity) {
            kinds.push(ResidualPredicate::Identifier);
        }
        if selectors.iter().any(|selector| selector.id_pattern.is_some()) {
            kinds.push(ResidualPredicate::IdentifierPattern);
        }
        if selectors.iter().any(|selector| selector.type_expr.is_some()) {
            kinds.push(ResidualPredicate::EntityType);
        }
        if !attrs.is_empty() {
            kinds.push(ResidualPredicate::Attributes);
        }
        if q_expr.is_some() {
            kinds.push(ResidualPredicate::ValueFilter);
        }
        if geo.is_some() {
            kinds.push(ResidualPredicate::GeoQuery);
        }
        if scope_q.is_some() {
            kinds.push(ResidualPredicate::ScopeQuery);
        }
        if projected_members.constrains() {
            kinds.push(ResidualPredicate::ProjectedMembers);
        }
        kinds
    }
}
