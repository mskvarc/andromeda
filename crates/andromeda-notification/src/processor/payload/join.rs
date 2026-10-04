//! The Linked Entities a subscription's `join` reaches from the notified
//! entities (clause 4.5.23). The walk that writes the notification nests them
//! for `inline` or writes them beside the entities for `flat`.

use crate::processor::payload::{
    NotificationBuildParams,
    cache::{LinkedEntities, LinkedKey, RenderingKey, resolve_linked_cached},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::{linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use indexmap::IndexMap;
use std::{collections::HashSet, num::NonZeroU32};
use tracing::{debug, warn};

/// The linked entities of the notified entity, the rendering `root` names, or
/// `None` when its links cannot be resolved, which does not fail the
/// notification: it goes out carrying the notified entity alone.
pub(super) async fn resolve_join_targets(
    main_entity: &Entity,
    root: &RenderingKey,
    params: &NotificationBuildParams<'_>,
    context: &ContextSpec,
) -> Option<LinkedEntities> {
    // clone: the linked key owns the key of the rendering it starts from
    let key = LinkedKey::new(root.clone(), params.sub.notification_join_level, params.sub.notification_sys_attrs);
    match resolve_linked_cached(params.target, params.entity_query, main_entity, key, context, params.cache).await {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a notification payload");
            None
        }
    }
}

/// The traversal one periodic notification asks for (clause 4.5.23).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PeriodicJoin<'a> {
    /// The branch the entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// How many levels are followed.
    pub(crate) join_level: NonZeroU32,
    /// Whether the linked entities carry their system attributes.
    pub(crate) sys_attrs: SysAttrsInclusion,
    /// The subscription's context the linked entities are compacted under.
    pub(crate) context: &'a ContextSpec,
}

/// The linked entities every entity of one periodic notification reaches,
/// none of them an entity the notification already carries, or `None` when
/// their links cannot be resolved, which leaves the notification with the
/// entities alone, as a change-driven one is left.
pub(crate) async fn resolve_periodic_join(entity_query: &EntityQueryService, roots: &[Entity], join: PeriodicJoin<'_>) -> Option<IndexMap<Identifier, Entity>> {
    match entity_query
        .resolve_linked_entities(join.target, roots, join.join_level, join.sys_attrs, join.context, &HashSet::new())
        .await
    {
        Ok(resolution) => {
            report_unresolved(&resolution.failures);
            Some(resolution.entities)
        }
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a periodic notification payload");
            None
        }
    }
}

/// Report every Linked Entity a notification's join left unresolved.
///
/// A notification has no response to carry a clause 6.3.17 warning on, so an
/// unresolvable Linked Entity is reported to the operator instead of the
/// subscriber, and the notification goes out with the entity simply absent.
pub(super) fn report_unresolved(failures: &[LinkedResolutionFailure]) {
    for failure in failures {
        warn!(
            entity = %failure.id,
            condition = %failure.kind,
            detail = %failure.detail,
            "linked entity left unresolved in a notification payload"
        );
    }
}
