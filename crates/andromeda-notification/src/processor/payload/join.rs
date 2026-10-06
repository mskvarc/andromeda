//! The Linked Entities a change-driven notification's `join` reaches from the
//! notified entity (clause 4.5.23). The walk that writes the notification
//! nests them for `inline` or writes them beside the entities for `flat`.

use crate::processor::payload::{
    NotificationBuildParams,
    cache::{LinkedEntities, LinkedKey, RenderingKey, cached_linked_graph},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::Entity;
use tracing::debug;

/// The linked entities of the notified entity, the rendering `root` names, or
/// `None` when its links cannot be resolved, which does not fail the
/// notification: it goes out carrying the notified entity alone.
pub(super) async fn notified_linked_graph(
    main_entity: &Entity,
    root: &RenderingKey,
    params: &NotificationBuildParams<'_>,
    context: &ContextSpec,
) -> Option<LinkedEntities> {
    // clone: the linked key owns the key of the rendering it starts from
    let key = LinkedKey::new(root.clone(), params.sub.notification_join_level, params.sub.notification_sys_attrs);
    match cached_linked_graph(params.target, params.entity_query, main_entity, key, context, params.cache).await {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a notification payload");
            None
        }
    }
}
