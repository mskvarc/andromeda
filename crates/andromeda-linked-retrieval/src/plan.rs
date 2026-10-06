//! What every writer renders: the Linked Entities of a joined payload, each
//! with every decision already taken.
//!
//! A flat join (clause 4.5.23.3) appends each reachable Linked Entity once, in
//! first-encounter order, narrowed by the list its first encounter decided. An
//! inline join (clause 4.5.23.2) nests a copy of the target under every
//! Relationship instance naming it, each narrowed by its own path's list: the
//! three peer brokers all do so, and it is the reading of "unless a URI has
//! been previously encountered" (clauses 5.7.1.5 and 5.7.2.5) that loses no
//! data. A target already open on the path from the root stays a plain URI,
//! since following it would loop (clause 4.5.23.1). In both shapes a Linked
//! Entity its list leaves no member is not written, and its Relationship keeps
//! the `object` that names it.
//!
//! A writer holds no state of its own: it iterates the flat list, or walks the
//! inline cursors the plan hands it, and asks a [`MemberRule`] for each member.

use crate::{
    graph::LinkedGraph,
    join::RetrievingJoin,
    narrowing::{encounter::FlatEncounters, member::MemberRule},
    representation::LinkedRepresentation,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use std::iter::successors;

/// The Linked Entities of a joined payload, in the shape the join names.
#[derive(Debug)]
pub enum JoinPlan<'g, R: LinkedRepresentation> {
    /// Appended beside the roots.
    Flat(FlatPlan<'g, R>),
    /// Nested under the Relationships naming them.
    Inline(InlinePlan<'g, R>),
}

impl<'g, R: LinkedRepresentation> JoinPlan<'g, R> {
    /// The plan rendering `graph` in the shape `join` names, the roots
    /// narrowed by `rule`.
    #[must_use]
    pub fn new(join: RetrievingJoin, graph: &'g LinkedGraph<R::Node>, rule: MemberRule<'g>) -> JoinPlan<'g, R> {
        let encounters = FlatEncounters::walk::<R>(graph, rule);
        match join {
            RetrievingJoin::Flat => JoinPlan::Flat(FlatPlan {
                linked: encounters
                    .written()
                    .filter_map(|(id, rule)| graph.nodes.get(id).map(|node| FlatEntity { id, node, rule }))
                    .collect(),
            }),
            RetrievingJoin::Inline => JoinPlan::Inline(InlinePlan { graph, rule, encounters }),
        }
    }

    /// The identifiers of the Linked Entities the payload carries, in
    /// first-encounter order (clause 5.5.14 records them beside the roots).
    #[must_use]
    pub fn linked_ids(&self) -> Vec<&'g Identifier> {
        match self {
            JoinPlan::Flat(flat) => flat.linked.iter().map(|linked| linked.id).collect(),
            JoinPlan::Inline(inline) => inline.encounters.written().map(|(id, _)| id).collect(),
        }
    }
}

/// The Linked Entities a flat join appends.
#[derive(Debug)]
pub struct FlatPlan<'g, R: LinkedRepresentation> {
    /// Each reachable, non-empty Linked Entity once, in first-encounter order.
    linked: Vec<FlatEntity<'g, R>>,
}

impl<'g, R: LinkedRepresentation> FlatPlan<'g, R> {
    /// The Linked Entities to append, in order.
    pub fn linked(&self) -> impl Iterator<Item = &FlatEntity<'g, R>> {
        self.linked.iter()
    }
}

/// One Linked Entity a flat join appends.
#[derive(Debug)]
pub struct FlatEntity<'g, R: LinkedRepresentation> {
    /// Its identifier.
    pub id: &'g Identifier,
    /// The entity.
    pub node: &'g R::Node,
    /// The rule its members are written by.
    pub rule: MemberRule<'g>,
}

/// The Linked Entities an inline join nests.
#[derive(Debug)]
pub struct InlinePlan<'g, R: LinkedRepresentation> {
    /// The traversed graph.
    graph: &'g LinkedGraph<R::Node>,
    /// The roots' rule.
    rule: MemberRule<'g>,
    /// The entities the payload reaches, for the identifiers it records.
    encounters: FlatEncounters<'g>,
}

impl<R: LinkedRepresentation> InlinePlan<'_, R> {
    /// The cursor every root is written from.
    #[must_use]
    pub const fn cursor(&self) -> InlineCursor<'_, R> {
        InlineCursor {
            graph: self.graph,
            rule: self.rule,
            depth: 0,
            path: None,
        }
    }
}

/// Where an inline writer stands: the rule of the entity it is about to
/// write, and the entities open above it.
#[derive(Debug)]
pub struct InlineCursor<'a, R: LinkedRepresentation> {
    /// The traversed graph.
    graph: &'a LinkedGraph<R::Node>,
    /// The rule of the entity about to be written.
    rule: MemberRule<'a>,
    /// How many hops below a root that entity sits.
    depth: u32,
    /// The entities open above it, innermost first.
    path: Option<&'a OpenPath<'a>>,
}

impl<'a, R: LinkedRepresentation> InlineCursor<'a, R> {
    /// The rule the entity about to be written is narrowed by.
    #[must_use]
    pub const fn rule(&self) -> MemberRule<'a> {
        self.rule
    }

    /// The entity `id` being written, open below the path.
    #[must_use]
    pub const fn open(self, id: &'a Identifier) -> OpenEntity<'a, R> {
        OpenEntity {
            cursor: self,
            path: Some(OpenPath { id, parent: self.path }),
        }
    }
}

// The derives would demand `R::Node: Clone` and `R::Node: Copy`, although the
// cursor only borrows the nodes; copying it copies references.
impl<R: LinkedRepresentation> Clone for InlineCursor<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: LinkedRepresentation> Copy for InlineCursor<'_, R> {}

/// One entity open while an inline writer writes its members.
#[derive(Debug)]
pub struct OpenEntity<'a, R: LinkedRepresentation> {
    /// Where the writer stands.
    cursor: InlineCursor<'a, R>,
    /// The path down to and including this entity, absent for a root
    /// written without its identifier.
    path: Option<OpenPath<'a>>,
}

impl<'a, R: LinkedRepresentation> OpenEntity<'a, R> {
    /// The copy of `target` to nest under this entity's attribute keyed
    /// `attribute`, with the cursor it is written from, or `None` when the
    /// target stays a plain URI: the attribute is not returned, the target was
    /// not resolved within the depth, it is open on the path (a loop), or its
    /// list leaves it no member.
    #[must_use]
    pub fn nested<'o>(&'o self, attribute: &str, target: &Uri) -> Option<(&'a R::Node, InlineCursor<'o, R>)> {
        let cursor = &self.cursor;
        if cursor.rule.member(attribute).is_skip() || cursor.depth >= cursor.graph.depth.get() || self.path.as_ref().is_some_and(|path| path.holds(target)) {
            return None;
        }
        let node = cursor.graph.nodes.get(target)?;
        let rule = cursor.rule.through(attribute);
        if rule.empties::<R>(node) {
            return None;
        }
        Some((
            node,
            InlineCursor {
                graph: cursor.graph,
                rule,
                depth: cursor.depth + 1,
                path: self.path.as_ref(),
            },
        ))
    }
}

/// The entities open on the path from a root down to the one being written,
/// innermost first.
#[derive(Debug, Clone, Copy)]
pub struct OpenPath<'a> {
    /// The entity being written.
    id: &'a Identifier,
    /// The path above it.
    parent: Option<&'a OpenPath<'a>>,
}

impl OpenPath<'_> {
    /// Whether `target` is open anywhere on the path.
    fn holds(&self, target: &Uri) -> bool {
        successors(Some(self), |path| path.parent).any(|path| path.id.as_uri() == target)
    }
}

#[cfg(test)]
mod tests;
