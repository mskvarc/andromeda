//! The write that stores a modified entity, and the notifications it owes.
//!
//! Every read-modify-write that neither creates nor deletes an entity
//! (merge, replace, the five attribute-level operations and the clause 5.6.21
//! purge) ends the same way: the state it computed is stored whole, its
//! change against the state it read is classified, and both notification
//! channels are planned from that one change (clause 5.2.12). A write that
//! modified nothing is not stored at all.

use crate::{
    EntityService,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    notification_plan::{WrittenEntity, modification, modified_sys_attrs},
    stored_census::admit_stored_entity,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::sync::Arc;

/// One computed modification: the write to commit and the events to publish
/// once it landed.
pub(crate) struct ModificationWrite {
    /// The write that stores the modified entity.
    pub(crate) write: AtomicWrite,
    /// In-process events matching the durable records committed with the write.
    pub(crate) events: Vec<ChangeEvent>,
}

/// Whether turning `previous` into `current` while recording `history` leaves
/// the entity exactly as it was: the state is the one that was read and
/// nothing is recorded in the Temporal Evolution.
///
/// Every write that may restate what the entity holds, single-entity and
/// batch alike, shares this test, so they cannot disagree on what a no-op is.
/// The history of a Core write observes only modified instances
/// (`Instance::same_content`), and stamping keeps the system times of an
/// instance restated unchanged, so a write restating every value the entity
/// holds passes it just as an empty Fragment does.
pub(crate) fn leaves_entity_unchanged(previous: &EntityState, current: &EntityState, history: &[HistoryRecord]) -> bool {
    history.is_empty() && previous == current
}

impl EntityService {
    /// The write that stores `current` as the entity `id` names in place of
    /// the state `read` holds, recording `history`, committed at `now`.
    ///
    /// The notifications it plans carry the entity's system attributes as
    /// this write leaves them: the `createdAt` of `read` and `now` as the
    /// `modifiedAt` the store records.
    ///
    /// No precondition is set here:
    /// [`commit_computed_write`](crate::EntityService::commit_computed_write)
    /// stamps it with the `modifiedAt` of the state the mutation was computed
    /// from, which is what stops a concurrent writer's change being
    /// overwritten. The durable matches are planned from the attempt that
    /// actually lands, so durable delivery describes the change the store
    /// accepted rather than one an earlier attempt computed and lost.
    ///
    /// # Errors
    ///
    /// Reports the [`EntityError`] of a state whose search projection cannot
    /// be built.
    pub(crate) fn modification_write(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        read: &EntitySnapshot,
        current: Arc<EntityState>,
        history: Vec<HistoryRecord>,
        now: SystemInstant,
    ) -> Result<ModificationWrite, EntityError> {
        let previous = &read.state;
        let change = modification(previous, &current);
        let sys_attrs = modified_sys_attrs(&read.sys_attrs, &current, now);
        let written = WrittenEntity {
            state: &current,
            sys_attrs: &sys_attrs,
        };
        let plan = self.notifier.plan(target, id, current.types().as_slice(), change, written, Some(previous));
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::Update(EntityRecord::replacing(previous, current)?))
            .history(history)
            .subscription_matches(plan.matches)
            .build();
        Ok(ModificationWrite { write, events: plan.events })
    }

    /// The write a modification owes for turning the state `read` holds into
    /// `current` at `now`, and the events it publishes, or no write and no
    /// events when it modified nothing.
    ///
    /// Clause 4.8 moves `modifiedAt` when the Entity "was last modified",
    /// clause 5.8.6 notifies a change only for a value "different than the
    /// existing one", and clause 4.3.1 updates the Temporal Evolution when the
    /// current state "is modified". A write whose state is the one it read and
    /// that records no history therefore stores nothing: the entity keeps its
    /// `modifiedAt`, no subscriber is notified and no Temporal Evolution
    /// instance is recorded. That holds for an empty Fragment (clause 5.4 makes
    /// a Fragment an IETF RFC 7396 JSON Merge Patch, which returns its target
    /// unchanged when it names nothing to change) and for a fragment or a
    /// replacement restating what the entity holds alike.
    ///
    /// A write that does modify the entity is held to the entity value limit
    /// on the state it stores, so writes each within the limit cannot grow
    /// one entity past it.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::PayloadBudget`] when the stored entity would
    /// pass the entity value limit, and the [`EntityError`] of
    /// [`EntityService::modification_write`].
    pub(crate) fn write_if_modified(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        read: &EntitySnapshot,
        current: Arc<EntityState>,
        history: Vec<HistoryRecord>,
        now: SystemInstant,
    ) -> Result<(Option<AtomicWrite>, Vec<ChangeEvent>), EntityError> {
        if leaves_entity_unchanged(&read.state, &current, &history) {
            return Ok((None, Vec::new()));
        }
        admit_stored_entity(self.payload_budget.json_values, Some(&read.state), &current)?;
        let ModificationWrite { write, events } = self.modification_write(target, id, read, current, history, now)?;
        Ok((Some(write), events))
    }
}
