//! A filter prepared once, for the many entity states it is run over.
//!
//! A Subscription's filter runs over every change its entity type sees, so
//! what depends on the filter alone is settled when the Subscription is
//! compiled: the attributes any match must hold, and the order the terms of
//! each group run in, cheapest first. A conjunction stops at its first term
//! that fails and a disjunction at its first that holds, and the answer of a
//! group does not depend on the order its members run in, so the order moves
//! cost and nothing else.

use crate::{
    q::{
        cost_class::CostClass,
        filter_run::term_holds,
        required_attributes::RequiredAttributes,
        term_memo::{InternerToken, TermInterner, TermMemo, TermNumber},
    },
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities},
};
use andromeda_ngsi_ld_query::filter::reading::{filter_reading::FilterReading, term_reading::TermReading};
use std::sync::Arc;

/// A filter's reading with its required attributes and the cost order of its
/// groups, which holds for exactly the entities the reading holds for.
#[derive(Debug, Clone)]
pub struct PreparedFilter {
    /// The attributes every entity the filter holds for must hold.
    required: RequiredAttributes,
    /// The boolean structure, every group in cost order.
    plan: Plan,
    /// The interner that numbered the terms, once they are numbered.
    numbered_by: Option<InternerToken>,
}

/// One node of the boolean structure and what running it costs.
#[derive(Debug, Clone)]
struct Plan {
    /// The costliest term beneath the node.
    cost: CostClass,
    /// What the node asks.
    shape: Shape,
}

/// The boolean structure of a prepared filter.
#[derive(Debug, Clone)]
enum Shape {
    /// Every member holds, the cheapest tried first.
    All(Vec<Plan>),
    /// Some member holds, the cheapest tried first.
    Any(Vec<Plan>),
    /// One Query Term.
    Term {
        /// The term.
        term: Box<TermReading>,
        /// Its number, once numbered: none for a term behind a hop, whose
        /// verdict depends on the entities the hop reads.
        number: Option<Arc<TermNumber>>,
    },
}

impl PreparedFilter {
    /// The preparation of `filter`.
    #[must_use]
    pub fn new(filter: &FilterReading) -> PreparedFilter {
        PreparedFilter {
            required: RequiredAttributes::of(filter),
            plan: Plan::of(filter),
            numbered_by: None,
        }
    }

    /// Gives every term that does not hop the number `interner` holds for it,
    /// so a [`TermMemo`] of that interner decides it once per pass.
    pub fn number_terms(&mut self, interner: &TermInterner) {
        self.plan.number_terms(interner);
        self.numbered_by = Some(interner.token());
    }

    /// The attributes every entity the filter holds for must hold.
    #[must_use]
    pub const fn required(&self) -> &RequiredAttributes {
        &self.required
    }

    /// Whether `entity` satisfies the filter, the entities its hops cross
    /// into read from `linked`.
    ///
    /// A state missing a required attribute is refused before any term runs,
    /// when there are several to check.
    pub fn holds<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(&self, entity: E, linked: L) -> bool {
        !self.lacks_required(entity) && self.plan.holds(entity, linked, None)
    }

    /// Whether `entity` lacks a required attribute, where checking them saves
    /// work.
    fn lacks_required<'a, E: QueriedEntity<'a>>(&self, entity: E) -> bool {
        self.required.saves_work() && !self.required.held_by(entity)
    }

    /// Whether `entity` satisfies the filter as [`PreparedFilter::holds`]
    /// says, a term numbered by the interner of `memo` decided once in the
    /// pass `memo` is in.
    ///
    /// The terms are decided over one entity and one source of linked
    /// entities for as long as the pass lasts. A filter numbered by another
    /// interner, or by none, is decided without the memo.
    pub fn holds_memoised<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(&self, entity: E, linked: L, memo: &mut TermMemo) -> bool {
        if self.lacks_required(entity) {
            return false;
        }
        let memo = (self.numbered_by == Some(memo.token())).then_some(memo);
        self.plan.holds(entity, linked, memo)
    }
}

impl Plan {
    /// The plan of `filter`, the members of every group ordered by cost.
    fn of(filter: &FilterReading) -> Plan {
        match filter {
            FilterReading::All(children) => Plan::group(children, Shape::All),
            FilterReading::Any(children) => Plan::group(children, Shape::Any),
            FilterReading::Term(term) => Plan {
                cost: CostClass::of(term),
                shape: Shape::Term {
                    term: term.clone(),
                    number: None,
                },
            },
        }
    }

    /// A group over `children`, cheapest first, whose members are put in
    /// `shape`.
    fn group(children: &[FilterReading], shape: fn(Vec<Plan>) -> Shape) -> Plan {
        let mut members: Vec<Plan> = children.iter().map(Plan::of).collect();
        members.sort_by_key(|member| member.cost);
        let cost = members.iter().map(|member| member.cost).max().unwrap_or(CostClass::Existence);
        Plan { cost, shape: shape(members) }
    }

    /// Numbers the terms of the node that can be memoised.
    fn number_terms(&mut self, interner: &TermInterner) {
        let cost = self.cost;
        match &mut self.shape {
            Shape::All(members) | Shape::Any(members) => members.iter_mut().for_each(|member| member.number_terms(interner)),
            Shape::Term { term, number } => *number = (cost != CostClass::Hop).then(|| interner.number(term)),
        }
    }

    /// Whether `entity` satisfies the node, a numbered term decided through
    /// `memo` when there is one.
    fn holds<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(&self, entity: E, linked: L, mut memo: Option<&mut TermMemo>) -> bool {
        match &self.shape {
            Shape::All(members) => members.iter().all(|member| member.holds(entity, linked, memo.as_deref_mut())),
            Shape::Any(members) => members.iter().any(|member| member.holds(entity, linked, memo.as_deref_mut())),
            Shape::Term { term, number } => match (number, memo) {
                (Some(number), Some(memo)) => memo.verdict(number.id(), || term_holds(term, entity, linked)),
                (Some(_), None) | (None, Some(_) | None) => term_holds(term, entity, linked),
            },
        }
    }
}
