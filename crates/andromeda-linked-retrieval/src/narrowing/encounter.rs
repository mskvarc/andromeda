//! Which Linked Entities a narrowed payload still reaches, and which encounter
//! decides how each is narrowed.
//!
//! Clause 5.7.1.5 brings a target along only through "the returned
//! Attributes", so a Relationship the list in force removes reaches nothing,
//! and whatever was reachable only through it leaves the payload. A flat join
//! writes each Linked Entity once, "unless a target URI has been previously
//! encountered" (clause 4.5.23.3), so the first encounter decides its list. The
//! walk is breadth-first, so the first encounter is the shallowest, which
//! leaves the entity's own Relationships the most `joinLevel` budget.

use crate::{edge::LinkedEdge, graph::LinkedGraph, narrowing::member::MemberRule, representation::LinkedRepresentation};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};

/// Whether a Linked Entity is written at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// It keeps a member under its list.
    Written,
    /// Its list leaves it no member, so it is not written, and none of its
    /// Relationships is returned to be walked.
    Emptied,
}

/// How one Linked Entity was first encountered.
#[derive(Debug, Clone, Copy)]
pub struct Encounter<'g> {
    /// The rule its members are written by.
    pub rule: MemberRule<'g>,
    /// Whether it is written.
    pub presence: Presence,
}

/// Every Linked Entity the narrowed payload reaches, in first-encounter
/// order.
#[derive(Debug)]
pub struct FlatEncounters<'g> {
    /// Each reached entity and its first encounter.
    encounters: IndexMap<&'g Identifier, Encounter<'g>>,
}

impl<'g> FlatEncounters<'g> {
    /// Walk `graph` breadth-first from its roots, narrowed by `rule`.
    ///
    /// Within one entity, edges are taken in member, instance and object
    /// order, and no entity is reached deeper than the graph was traversed.
    #[must_use]
    pub fn walk<R: LinkedRepresentation>(graph: &'g LinkedGraph<R::Node>, rule: MemberRule<'g>) -> FlatEncounters<'g> {
        let mut leaving: HashMap<&Identifier, Vec<&LinkedEdge>> = HashMap::new();
        for edge in &graph.edges {
            leaving.entry(&edge.from).or_default().push(edge);
        }
        let roots: HashSet<&Identifier> = graph.roots.iter().collect();
        let mut queue: VecDeque<(&Identifier, MemberRule<'g>, u32)> = graph.roots.iter().map(|root| (root, rule, 0)).collect();
        let mut encounters: IndexMap<&'g Identifier, Encounter<'g>> = IndexMap::new();

        while let Some((entity, rule, depth)) = queue.pop_front() {
            if depth >= graph.depth.get() {
                continue;
            }
            for edge in leaving.get(entity).into_iter().flatten() {
                let attribute = edge.attribute.as_str();
                if rule.member(attribute).is_skip() || roots.contains(&edge.target) || encounters.contains_key(&edge.target) {
                    continue;
                }
                let Some(node) = graph.nodes.get(&edge.target) else {
                    continue;
                };
                let target_rule = rule.through(attribute);
                let presence = if target_rule.empties::<R>(node) {
                    Presence::Emptied
                } else {
                    Presence::Written
                };
                encounters.insert(&edge.target, Encounter { rule: target_rule, presence });
                if presence == Presence::Written {
                    queue.push_back((&edge.target, target_rule, depth + 1));
                }
            }
        }
        FlatEncounters { encounters }
    }

    /// Every written Linked Entity with the rule its first encounter decided,
    /// in first-encounter order.
    pub fn written(&self) -> impl Iterator<Item = (&'g Identifier, MemberRule<'g>)> + '_ {
        self.encounters
            .iter()
            .filter(|(_, encounter)| encounter.presence == Presence::Written)
            .map(|(id, encounter)| (*id, encounter.rule))
    }
}

#[cfg(test)]
mod tests;
