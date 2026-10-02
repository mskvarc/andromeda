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
}
