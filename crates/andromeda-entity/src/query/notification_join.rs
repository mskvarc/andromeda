//! The Linked Entities a subscription's `join` reaches from the entities one
//! notification carries (clause 4.5.23, clause 5.8.6), read from this broker's
//! store by the engine a query's `join` reads them through.
//!
//! Every notification that carries entities the broker holds or relays asks
//! here, so a subscription's `join` writes the same Linked Entities whichever
//! way the notified entities arrived.

use crate::query::{EntityQueryService, core_linked_source::CoreLinkedSource};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::TraversalBounds,
    narrowing::list::ListInForce,
    representation::LinkedRepresentation,
    traversal::traverse,
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use andromeda_ngsi_ld_representation::{linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use std::num::NonZeroU32;
use tracing::{debug, warn};

/// The traversal one notification asks for (clause 4.5.23).
#[derive(Clone, Copy, Debug)]
pub struct NotificationJoin<'a> {
    /// The branch the entities are read from.
    pub target: &'a ContextTarget,
    /// How many levels are followed.
    pub join_level: NonZeroU32,
    /// Whether the linked entities carry their system attributes.
    pub sys_attrs: SysAttrsInclusion,
    /// The subscription's context the linked entities are compacted under.
    pub context: &'a ContextSpec,
}

/// The linked entities every entity of `roots` reaches, none of them an entity
/// the notification already carries, or `None` when their links cannot be
/// resolved, which leaves the notification with the entities alone.
pub async fn notification_linked_graph(entity_query: &EntityQueryService, roots: &[Entity], join: NotificationJoin<'_>) -> Option<LinkedGraph<Entity>> {
    let source = CoreLinkedSource::new(entity_query, join.target, join.sys_attrs, join.context);
    let bounds = TraversalBounds::of(Some(join.join_level), None);
    match traverse(&source, RootEdges::of::<TypedEntities>(roots), &bounds).await {
        Ok(graph) => {
            report_unresolved(&graph.failures);
            Some(graph)
        }
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a notification payload");
            None
        }
    }
}

/// The expansion of every attribute key the notified `roots` and the `linked`
/// entities carry, under the subscription's `context` they were compacted
/// under, which the subscription's `list` is matched against while the Linked
/// Entities are planned and written (clause 5.8.6).
///
/// A list that narrows nothing matches nothing, and needs no index. `None`
/// when a key cannot be expanded, which leaves the notification without its
/// Linked Entities, as a traversal that fails leaves it.
pub async fn linked_attribute_index<'e>(
    engine: &JsonLdEngine,
    context: &ContextSpec,
    list: ListInForce<'_>,
    roots: impl Iterator<Item = &'e Entity>,
    linked: &'e LinkedGraph<Entity>,
) -> Option<AttributeIriIndex> {
    if list.is_unrestricted() {
        return Some(AttributeIriIndex::default());
    }
    let prepared = engine.prepare_term_context(context);
    let expander = ContextualExpander::new(engine, &prepared);
    // Collected so the keys borrow no longer than the expander they are
    // expanded through.
    let keys: Vec<&str> = roots
        .flat_map(TypedEntities::attribute_keys)
        .chain(linked.attribute_keys::<TypedEntities>())
        .collect();
    match AttributeIriIndex::of_keys(keys, &expander).await {
        Ok(index) => Some(index),
        Err(error) => {
            debug!(%error, "the attribute keys of a notification's Linked Entities could not be expanded");
            None
        }
    }
}

/// Report every Linked Entity a notification's join left unresolved.
///
/// A notification has no response to carry a clause 6.3.17 warning on, so an
/// unresolvable Linked Entity is reported to the operator instead of the
/// subscriber, and the notification goes out with the entity simply absent.
pub fn report_unresolved(failures: &[LinkedResolutionFailure]) {
    for failure in failures {
        warn!(
            entity = %failure.id,
            condition = %failure.kind,
            detail = %failure.detail,
            "linked entity left unresolved in a notification payload"
        );
    }
}
