//! Preparing a batch whose entities name different contexts.
//!
//! JSON-LD applies a top-level node's `@context` to that node alone, so each
//! entity of an `application/ld+json` batch is expanded under the context it
//! carried itself (clause 5.5.7, "the @context [...] provided by each API
//! call"). Entities naming one and the same context, one
//! [`BatchContextId`](andromeda_jsonld::batch_contexts::BatchContextId), given
//! when the batch was parsed, are prepared together, so a batch whose
//! entities agree processes its context once, and a batch whose entities
//! differ never reads one entity's terms through another's definitions.

use crate::{
    EntityService,
    batch::{
        BatchMember,
        prepare::{PendingEntity, PreparationOutcome},
    },
    error::EntityError,
    write_admission::BatchWrite,
};
use andromeda_jsonld::batch_contexts::{BatchContext, ContextGroups};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;

/// An entity its preparation expanded and screened, with the context it was
/// expanded under.
pub(super) struct ExpandedEntity<M> {
    /// The entity as submitted.
    pub entity: Entity<M>,
    /// What it expanded to.
    pub document: EntityDocument,
    /// The context it was expanded under.
    pub context: BatchContext,
}

/// What preparing a batch produced: one slot per position, filled where
/// preparation succeeded, and the failures against their identifiers.
pub(super) struct PreparedBatch<M> {
    /// The prepared entity at each position.
    pub slots: Vec<Option<ExpandedEntity<M>>>,
    /// The per-entity failures, in position order.
    pub errors: Vec<(Identifier, EntityError)>,
}

impl EntityService {
    /// Prepare every group under its own context for the batch operation
    /// `write`, answering one slot per position below `slot_count`.
    ///
    /// Each entry's `idx` is its position in the whole batch, so the groups'
    /// slots interleave back into submission order and the failures come back
    /// in that order too, each against its own entity's identifier. A position
    /// no entry names stays empty.
    pub(super) async fn prepare_grouped<M: BatchMember>(
        &self,
        slot_count: usize,
        groups: ContextGroups<PendingEntity<M>>,
        write: BatchWrite,
    ) -> PreparedBatch<M> {
        let execution = self.batch_execution(slot_count);
        let screening = self.screening(write);
        let mut outcomes: Vec<Option<(PreparationOutcome<M>, BatchContext)>> = (0..slot_count).map(|_| None).collect();
        for (context, entries) in groups {
            for (idx, preparation) in self.prepare_group(entries, context.spec(), execution, screening).await {
                if let Some(slot) = outcomes.get_mut(idx) {
                    *slot = Some((preparation, context.clone())); // clone: each prepared entity holds the shared context handle
                }
            }
        }
        let mut errors = Vec::new();
        let slots = outcomes
            .into_iter()
            .map(|outcome| match outcome {
                Some((Ok((entity, document)), context)) => Some(ExpandedEntity { entity, document, context }),
                Some((Err(failure), _)) => {
                    errors.push(*failure);
                    None
                }
                None => None,
            })
            .collect();
        PreparedBatch { slots, errors }
    }
}
