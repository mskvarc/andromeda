//! The broker's own side-effect drain over the suite's database, in live
//! delivery, with the changes it publishes recorded.

use crate::{broker_committer::committer, queue_database::side_effect_queue, recorded_changes::RecordedChanges};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{
    change_sink::EntityChangeSink,
    side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainConfig},
    side_effect_notification::SideEffectNotificationRoute,
    side_effect_queue::SideEffectQueue,
};
use sqlx::PgPool;
use std::sync::Arc;

/// A drain over the suite's queue replaying through the broker's committer
/// under `config`, and the record of the changes it publishes.
pub fn live_side_effect_drain(pool: &PgPool, config: EntitySideEffectDrainConfig) -> (Arc<EntitySideEffectDrain>, Arc<RecordedChanges>) {
    let published = Arc::new(RecordedChanges::default());
    let drain = EntitySideEffectDrain::new(
        Arc::new(side_effect_queue(pool)) as Arc<dyn SideEffectQueue>,
        Arc::new(committer(pool)) as Arc<dyn ContextCommitter>,
        SideEffectNotificationRoute::Live(Arc::clone(&published) as Arc<dyn EntityChangeSink>),
        config,
    );
    (Arc::new(drain), published)
}
