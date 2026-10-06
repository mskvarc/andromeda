//! The write that stores a modified entity, and the notifications it owes.
//!
//! Every read-modify-write that neither creates nor deletes an entity --
//! merge, replace, the five attribute-level operations and the clause 5.6.21
//! purge -- ends the same way: the state it computed is stored whole, its
//! change against the state it read is classified, and both notification
//! channels are planned from that one change (clause 5.2.12).

use crate::{EntityService, error::EntityError, notification_plan::modification};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
/// Single-entity fragment operations and the batch operations that run them
/// per entity share this test, so they cannot disagree on what a no-op is.
pub(crate) fn leaves_entity_unchanged(previous: &EntityState, current: &EntityState, history: &[HistoryRecord]) -> bool {
    history.is_empty() && previous == current
}

impl EntityService {
    /// The write that stores `current` as the entity `id` names in place of
    /// `previous`, recording `history`.
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
        previous: &Arc<EntityState>,
        current: Arc<EntityState>,
        history: Vec<HistoryRecord>,
    ) -> Result<ModificationWrite, EntityError> {
        let change = modification(previous, &current);
        let plan = self
            .notifier
            .plan(target, id, current.types().as_slice(), change, Some(&current), Some(previous));
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::Update(EntityRecord::replacing(previous, current)?))
            .history(history)
            .subscription_matches(plan.matches)
            .build();
        Ok(ModificationWrite { write, events: plan.events })
    }

    /// The write a fragment operation owes for turning `previous` into
    /// `current`, and the events it publishes, or no write and no events when
    /// it changed nothing.
    ///
    /// Clause 5.4 makes a Fragment an IETF RFC 7396 JSON Merge Patch, which
    /// returns its target unchanged when it names nothing to change, and
    /// clauses 5.5.8 and 5.5.12 iterate over the members of the Fragment. A
    /// write whose state is the one it read and that records no history
    /// therefore stores nothing: the entity keeps its `modifiedAt` (clause 4.8),
    /// no subscriber is notified and no Temporal Evolution instance is
    /// recorded.
    ///
    /// # Errors
    ///
    /// Reports the [`EntityError`] of [`EntityService::modification_write`].
    pub(crate) fn fragment_write(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        previous: &Arc<EntityState>,
        current: Arc<EntityState>,
        history: Vec<HistoryRecord>,
    ) -> Result<(Option<AtomicWrite>, Vec<ChangeEvent>), EntityError> {
        if leaves_entity_unchanged(previous, &current, &history) {
            return Ok((None, Vec::new()));
        }
        let ModificationWrite { write, events } = self.modification_write(target, id, previous, current, history)?;
        Ok((Some(write), events))
    }
}
