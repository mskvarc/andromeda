//! Batch entity deletion (clause 5.6.10).
//!
//! A deleted entity's payload and system attributes are unreadable once the row
//! is gone, and the clause 5.6.10 notifications carry both, so both are read
//! before the delete, which the tombstones of clause 5.6.6 need as well. The
//! tombstones and the notification are only true of the state they were read
//! from, so each delete holds only while the entity is unchanged since that
//! read and is rebuilt from a fresh one when it is not (see
//! [`crate::batch::chunk_read_modify_write`]): an instance a concurrent append
//! added is ended in the Temporal Evolution like every other.

use crate::{
    EntityService,
    batch::{
        chunk::ChunkWrite,
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    expanded_member::stored_entity_types,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::{NotificationPlan, deletion},
    side_effect::SideEffectPlacement,
    snapshot_supply::SnapshotSupply,
};
use andromeda_context_commit::{
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    history_record::deletion_history,
    precondition::Precondition,
    subscription_match::SubscriptionMatch,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, SysAttrsSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// One chunk's successfully deleted identifiers and its per-entity failures.
pub(super) type DeleteChunkOutcome = (Vec<Identifier>, Vec<(Identifier, EntityError)>);

/// Deleting each entity of a chunk as an attempt read it (clause 5.6.10.4 runs
/// the behaviour of clause 5.6.6 per entity).
struct DeleteRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for DeleteRewrite<'_> {
    type Entry = Identifier;
    /// The system attributes of every pending entity: the durable matches and
    /// the notifications carry them, and they are unreadable once the row is
    /// gone.
    type Reads = HashMap<Identifier, SysAttrsSnapshot>;

    fn id(entry: &Identifier) -> &Identifier {
        entry
    }

    async fn read(&self, pending: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        self.service.reader.get_sys_attrs_batch(self.target, pending).await
    }

    fn build(
        &self,
        entry: Identifier,
        snapshot: Option<&EntitySnapshot>,
        reads: &HashMap<Identifier, SysAttrsSnapshot>,
        _now: DateTime<Utc>,
    ) -> Result<PlannedWrite<Identifier>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let error = EntityError::not_found(&entry);
            return Err(Box::new((entry, error)));
        };
        let payload = &snapshot.payload;
        let sys_attrs = reads.get(&entry);
        let built = deletion_events(&entry, payload, sys_attrs).and_then(|events| {
            let matches = self.service.delete_subscription_matches(self.target, &entry, payload, sys_attrs)?;
            Ok((NotificationPlan { matches, events }, deletion_history(payload)?))
        });
        let (plan, history) = match built {
            Ok(built) => built,
            Err(error) => return Err(Box::new((entry, error))),
        };
        Ok(PlannedWrite {
            write: ChunkWrite {
                id: entry.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
                entity: EntityMutation::Delete,
                // Clause 5.6.6 removes current state and leaves the Temporal
                // Evolution standing, so every instance the entity still held is
                // recorded as ended rather than being deleted with it.
                history,
                precondition: Precondition::MustExist,
                plan,
            },
            entry,
            alongside: None,
        })
    }
}

impl EntityService {
    /// The durable notification records one deletion owes, mirroring the single
    /// delete (clause 5.6.6): one per subscription the deletion is owed to,
    /// carrying the pre-delete snapshot as its entity and previous image.
    ///
    /// The entity snapshot the durable match stores has the clause-4.8 system
    /// attributes embedded when they are known, because the deleted row is gone
    /// by delivery time and the durable path cannot re-read `createdAt`/
    /// `modifiedAt` the way a non-deletion notification does.
    fn delete_subscription_matches(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        payload: &Arc<Value>,
        sys_attrs: Option<&SysAttrsSnapshot>,
    ) -> Result<Vec<SubscriptionMatch>, EntityError> {
        let entity_types = stored_entity_types(payload)?;
        let change = deletion(payload)?;
        let deleted = ChangedEntity {
            target,
            id,
            types: &entity_types,
        };
        let snapshots = MutationSnapshots {
            current: SnapshotSupply::Removed { removed: payload, sys_attrs },
            previous: Some(payload),
        };
        Ok(self.notifier.subscription_matches(deleted, &change, snapshots))
    }

    pub(super) async fn batch_delete_chunk(&self, placement: SideEffectPlacement<'_>, chunk: &[Identifier]) -> Result<DeleteChunkOutcome, EntityError> {
        let rewrite = DeleteRewrite {
            service: self,
            target: placement.target(),
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, chunk.to_vec()).await?;
        Ok((outcome.success, outcome.errors))
    }
}

/// The clause 5.6.10 notification one deletion raises: one change carrying
/// the Entity's deletion and every instance it took with it (clause 5.8.6).
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the stored payload's types or
/// `datasetId`s cannot be read.
fn deletion_events(id: &Identifier, payload: &Arc<Value>, sys_attrs: Option<&SysAttrsSnapshot>) -> Result<Vec<ChangeEvent>, EntityError> {
    Ok(vec![
        ChangeEvent::builder()
            .entity_id(id.clone()) // clone: the event owns its identifier
            .entity_types(stored_entity_types(payload)?)
            .change(deletion(payload)?)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .entity_snapshot(Some(Arc::clone(payload)))
            .previous_attributes(Some(Arc::clone(payload)))
            .sys_attrs_snapshot(sys_attrs.cloned())
            .build(),
    ])
}
