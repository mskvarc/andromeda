//! The Linked Entities a subscription's `join` reaches from the notified
//! entity (clause 4.5.23). The walk that writes the notification nests them
//! for `inline` or writes them beside the entity for `flat`.

use crate::processor::payload::{
    NotificationBuildParams,
    cache::{LinkedEntities, resolve_linked_cached},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::Entity;
use tracing::debug;

/// The linked entities of the notified entity, or `None` when its links cannot
/// be resolved, which does not fail the notification: it goes out carrying
/// the notified entity alone.
pub(super) async fn resolve_join_targets(main_entity: &Entity, params: &NotificationBuildParams<'_>, context: &ContextSpec) -> Option<LinkedEntities> {
    match resolve_linked_cached(
        params.target,
        params.entity_query,
        main_entity,
        params.sub.notification_join_level,
        params.sub.notification_sys_attrs,
        (context, params.sub.jsonld_context.as_ref()),
        params.cache,
    )
    .await
    {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a notification payload");
            None
        }
    }
}
