//! Batch entity deletion (clause 5.6.10).
//!
//! A deleted entity's payload and system attributes are unreadable once the row
//! is gone, and the clause 5.6.10 notifications carry both, so both are read,
//! in one snapshot, before the delete, which the tombstones of clause 5.6.6
//! need as well. The
//! tombstones and the notification are only true of the state they were read
//! from, so each delete holds only while the entity is unchanged since that
//! read and is rebuilt from a fresh one when it is not (see
//! [`crate::batch::chunk_read_modify_write`]): an instance a concurrent append
//! added is ended in the Temporal Evolution like every other.

use crate::{
    EntityService,
    batch::{
        chunk::ChunkWrite,
        chunk_read_modify_write::{ChunkRead, ChunkRewrite, PlannedWrite},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::{NotificationPlan, deletion},
    side_effect::SideEffectPlacement,
    snapshot_supply::SnapshotSupply,
};
use andromeda_context_commit::{
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    history_record::deletion_history,
    subscription_match::SubscriptionMatch,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, SysAttrsSnapshot};
use chrono::Utc;
use std::sync::Arc;

/// One chunk's successfully deleted identifiers and its per-entity failures.
pub(super) type DeleteChunkOutcome = (Vec<Identifier>, Vec<(Identifier, EntityError)>);

/// Deleting each entity of a chunk as an attempt read it (clause 5.6.10.4 runs
/// the behaviour of clause 5.6.6 per entity).
struct DeleteRewrite<'a> {
    /// The service whose deletion builds each write.
    service: &'a EntityService,
    /// The branch the chunk writes.
    target: &'a ContextTarget,
}

impl ChunkRewrite for DeleteRewrite<'_> {
    type Entry = Identifier;

    const READ: ChunkRead = ChunkRead::Snapshots;

    fn id(entry: &Identifier) -> &Identifier {
        entry
    }

    fn build(
        &self,
        entry: Identifier,
        snapshot: Option<&EntitySnapshot>,
        _now: SystemInstant,
    ) -> Result<PlannedWrite<Identifier>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let error = EntityError::not_found(&entry);
            return Err(Box::new((entry, error)));
        };
        let removed = &snapshot.state;
        let sys_attrs = &snapshot.sys_attrs;
        let plan = NotificationPlan {
            matches: self.service.delete_subscription_matches(self.target, &entry, removed, sys_attrs),
            events: deletion_events(&entry, removed, sys_attrs),
        };
        Ok(PlannedWrite {
            write: Some(ChunkWrite {
                id: entry.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
                entity: EntityMutation::Delete,
                // Clause 5.6.6 removes current state and leaves the Temporal
                // Evolution standing, so every instance the entity still held is
                // recorded as ended rather than being deleted with it.
                history: deletion_history(removed),
                plan,
            }),
            entry,
        })
    }
}

impl EntityService {
    /// The durable notification records one deletion owes, mirroring the single
    /// delete (clause 5.6.6): one per subscription the deletion is owed to,
    /// carrying the removed state as its entity and previous image.
    ///
    /// The clause 4.8 system attributes travel beside the removed state,
    /// because the deleted row is gone by delivery time.
    fn delete_subscription_matches(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        removed: &Arc<EntityState>,
        sys_attrs: &SysAttrsSnapshot,
    ) -> Vec<SubscriptionMatch> {
        let deleted = ChangedEntity {
            target,
            id,
            types: removed.types().as_slice(),
        };
        let snapshots = MutationSnapshots {
            current: SnapshotSupply::Carried {
                state: removed,
                sys_attrs: Some(sys_attrs),
            },
            previous: Some(removed),
        };
        self.notifier.subscription_matches(deleted, &deletion(removed), snapshots)
    }

    /// Delete the entities `chunk` names at `now`, returning the deleted identifiers
    /// and the per-entity failures. Fails only on a store failure that concerns the whole chunk.
    pub(super) async fn batch_delete_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: &[Identifier],
        now: SystemInstant,
    ) -> Result<DeleteChunkOutcome, EntityError> {
        let rewrite = DeleteRewrite {
            service: self,
            target: placement.target(),
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, chunk.to_vec(), now).await?;
        Ok((outcome.success, outcome.errors))
    }
}

/// The clause 5.6.10 notification one deletion raises: one change carrying
/// the Entity's deletion and every instance it took with it (clause 5.8.6).
fn deletion_events(id: &Identifier, removed: &Arc<EntityState>, sys_attrs: &SysAttrsSnapshot) -> Vec<ChangeEvent> {
    vec![
        ChangeEvent::builder()
            .entity_id(id.clone()) // clone: the event owns its identifier
            .entity_types(removed.types().as_slice().to_vec()) // clone: the event owns the names of the types it carries
            .change(deletion(removed))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .entity_snapshot(Some(Arc::clone(removed))) // clone: reference-count bump
            .previous_attributes(Some(Arc::clone(removed))) // clone: reference-count bump
            .sys_attrs_snapshot(Some(sys_attrs.clone())) // clone: the event owns the system times it carries
            .build(),
    ]
}
