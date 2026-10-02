//! The compaction of one notified entity under the subscription's `@context`
//! (clause 5.8.6).
//!
//! Shared by the change-driven and the periodic notification paths, which
//! build their entities differently but deliver them the same way.

use crate::error::NotificationError;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::{
    compaction_error::CompactionError,
    entity_compactor::{EntityCompactor, EntityRead},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::entity::Entity;
use tracing::debug;

/// The compact entity `state` is under the subscription's context, the one
/// `compactor` compacts against, falling back to the core context when that
/// context cannot be applied.
///
/// The entity carries no `@context`: a notification carries one context for
/// the whole document (clauses 5.2.13 and 6.3.5), which the delivery request
/// adds when the media type is `application/ld+json`.
///
/// # Errors
///
/// Returns the core-context failure when neither context can compact the
/// entity, which leaves nothing to deliver.
pub(crate) async fn compact_notified_state(
    compactor: &EntityCompactor<'_>,
    engine: &JsonLdEngine,
    state: &EntityState,
    read: &EntityRead<'_>,
) -> Result<Entity, NotificationError> {
    match compactor.entity(state, read).await {
        Ok(entity) => Ok(entity),
        Err(error) => {
            debug!(%error, "Failed to compact notification entity with subscription context; falling back to the core context");
            Ok(core_compaction(engine, state, read).await?)
        }
    }
}

/// The compact entity `state` is under the core context alone.
async fn core_compaction(engine: &JsonLdEngine, state: &EntityState, read: &EntityRead<'_>) -> Result<Entity, CompactionError> {
    let core_only = ContextSpec::core_only();
    let compactor = engine.compactor(&core_only);
    EntityCompactor::new(&compactor).entity(state, read).await
}
