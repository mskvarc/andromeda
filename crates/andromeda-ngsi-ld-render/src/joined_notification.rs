//! The entities of one notification once a subscription's `join` has reached
//! its Linked Entities (clause 4.5.23, clause 5.8.6): nested into the notified
//! entities for `inline`, written after all of them for `flat`.
//!
//! Notified entities arrive in more than one form (a change the broker
//! committed, an entity a context source relayed), so the caller says how each
//! is written and this module decides where the Linked Entities go, which is
//! one decision whichever way the entities came.

use crate::notification_entity::NotificationEntity;
use andromeda_linked_retrieval::{
    graph::LinkedGraph,
    join::RetrievingJoin,
    narrowing::member::MemberRule,
    plan::{FlatEntity, InlineCursor, JoinPlan},
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::join_mode::JoinMode;

/// The plan the subscription's `join` makes of `graph`, the roots narrowed by
/// `rule`; `None` when the subscription names no retrieving join or no
/// traversal ran, which writes the notified entities alone.
#[must_use]
pub fn join_plan<'a>(join: Option<JoinMode>, graph: Option<&'a LinkedGraph<Entity>>, rule: MemberRule<'a>) -> Option<JoinPlan<'a, TypedEntities>> {
    RetrievingJoin::from_mode(join)
        .zip(graph)
        .map(|(join, graph)| JoinPlan::<TypedEntities>::new(join, graph, rule))
}

/// Every entity of the notification, in order: the `notified` ones, written by
/// `write` with the cursor an inline `plan` hands them, then the Linked
/// Entities a flat `plan` appends, written by `linked`.
#[must_use]
pub fn joined_entities<'a, N>(
    plan: Option<&'a JoinPlan<'a, TypedEntities>>,
    notified: &'a [N],
    write: impl Fn(&'a N, Option<InlineCursor<'a, TypedEntities>>) -> NotificationEntity<'a>,
    linked: impl Fn(&FlatEntity<'a, TypedEntities>) -> NotificationEntity<'a>,
) -> Vec<NotificationEntity<'a>> {
    match plan {
        Some(JoinPlan::Inline(inline)) => notified.iter().map(|entity| write(entity, Some(inline.cursor()))).collect(),
        Some(JoinPlan::Flat(flat)) => notified.iter().map(|entity| write(entity, None)).chain(flat.linked().map(linked)).collect(),
        None => notified.iter().map(|entity| write(entity, None)).collect(),
    }
}
