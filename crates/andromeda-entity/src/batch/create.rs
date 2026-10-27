//! Batch entity creation (clause 5.6.7).

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::{AdmittedBatchEntity, prepared_chunk},
        chunk::GuardedWrite,
    },
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_commit::precondition::Precondition;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use std::collections::HashSet;

impl EntityService {
    /// Reject the second and later appearances of a repeated identifier.
    ///
    /// A batch that names one entity twice is asking to create it twice, and
    /// the second is a conflict exactly as it would be in a separate request
    /// (NGSI-LD 6.4.3.1).
    pub(super) fn dedupe_create_entities(entities: Vec<AdmittedBatchEntity>) -> (Vec<AdmittedBatchEntity>, Vec<(Identifier, EntityError)>) {
        let first_instances = {
            let mut seen = HashSet::with_capacity(entities.len());
            entities.iter().map(|entity| seen.insert(entity.id())).collect::<Vec<_>>()
        };
        let mut deduped = Vec::with_capacity(entities.len());
        let mut errors = Vec::new();
        for (entity, first) in entities.into_iter().zip(first_instances) {
            if first {
                deduped.push(entity);
            } else {
                errors.push((entity.id().clone(), EntityError::already_exists(entity.id()))); // clone: error result owns the entity ID
            }
        }

        (deduped, errors)
    }

    pub(super) async fn batch_create_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        now: SystemInstant,
    ) -> Result<BatchResult, EntityError> {
        let target = placement.target();
        // Clause 5.6.7.4 runs the behaviour of Create Entity for each item, and
        // clause 5.5.4 excepts only the fragments of a partial update or a
        // merge from its NGSI-LD Null prohibitions.
        let prepared = prepared_chunk(chunk);
        let mut errors = Vec::new();

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let lock_ids: Vec<Identifier> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: guards outlive the borrowed preparation view
        let _id_guards = self.acquire_id_locks(target, &lock_ids).await;

        let mut seen: HashSet<Identifier> = HashSet::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            // clone: the set remembers the identifier the entity moves on with
            if !seen.insert(entity.id.clone()) {
                errors.push((entity.id.clone(), EntityError::already_exists(&entity.id))); // clone: error result owns the entity ID
                continue;
            }
            // A batch insert holds only while the entity does not exist, so a
            // concurrent create answers 409 for this entity alone.
            match entity.into_state().and_then(|(id, state)| self.chunk_insert(target, id, state, now)) {
                Ok(write) => writes.push(GuardedWrite {
                    write,
                    precondition: Precondition::MustNotExist,
                }),
                Err(failure) => errors.push(*failure),
            }
        }

        let outcome = self.commit_chunk(placement, now, writes).await?.settled();
        errors.extend(outcome.errors);

        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
