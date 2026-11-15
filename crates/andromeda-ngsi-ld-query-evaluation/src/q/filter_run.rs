//! Running the reading of a `q` filter against what is held in memory (clause
//! 4.9).
//!
//! The reading took every clause 4.9 decision, from which instance member a
//! path ends on to what a target of each kind makes of a test. What is left
//! to run is the walk to the targets a term's path addresses, the
//! [`attribute_axis`](crate::q::attribute_axis)'s, and the test of each one,
//! the [`verdict_run`](crate::q::verdict_run)'s.

use crate::{
    q::{
        attribute_axis::{addresses_element, resolve_targets},
        verdict_run::target_passes,
    },
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities, resource_members::ResourceMembers},
};
use andromeda_ngsi_ld_query::filter::reading::{
    filter_reading::FilterReading,
    term_reading::{TermReading, Test},
};

/// Whether an entity satisfies a filter's reading, the entities its hops
/// cross into read from `linked`.
///
/// The reading's attribute names are expanded, as the entity keys its
/// attributes by expanded name.
pub fn filter_holds<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(filter: &FilterReading, entity: E, linked: L) -> bool {
    holds_by(filter, &|term| term_holds(term, entity, linked))
}

/// Whether an entity satisfies one Query Term on its own, the entities its
/// hops cross into read from `linked`.
///
/// Every Query Term is existential over the targets its path addresses, which
/// is what lets a filter over an entity held in several parts be decided term
/// by term (clause 5.7.4.4 `splitEntities`): the term holds over the whole
/// entity when it holds over any one part.
pub fn term_holds<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(term: &TermReading, entity: E, linked: L) -> bool {
    match term.test() {
        // Clause 4.9: existence holds when the entity "define[s] the target
        // element", whatever attribute family it belongs to and whatever value
        // it holds.
        Test::Exists => addresses_element(entity, term, linked),
        Test::Value(test) => resolve_targets(entity, term, linked).iter().any(|target| target_passes(test, target)),
    }
}

/// Whether a resource that is not an entity satisfies a filter's reading,
/// each term read from the one member it names.
///
/// Clause 4.9 makes every operator require the target element, `unequal` and
/// `notPatternOp` included, so a member the resource does not hold matches no
/// comparison.
pub fn members_hold<'t, M: ResourceMembers<'t>>(filter: &FilterReading, resource: M) -> bool {
    holds_by(filter, &|term| match term.test() {
        Test::Exists => resource.member_target(term).is_some(),
        Test::Value(test) => resource.member_target(term).is_some_and(|target| target_passes(test, &target)),
    })
}

/// Whether the boolean structure of `filter` holds over terms that hold as
/// `term_holds` says.
fn holds_by(filter: &FilterReading, term_holds: &impl Fn(&TermReading) -> bool) -> bool {
    match filter {
        FilterReading::All(children) => children.iter().all(|child| holds_by(child, term_holds)),
        FilterReading::Any(children) => children.iter().any(|child| holds_by(child, term_holds)),
        FilterReading::Term(term) => term_holds(term),
    }
}
