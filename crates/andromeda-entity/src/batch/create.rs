//! Batch entity creation (clause 5.6.7).

use crate::{
    EntityService,
    batch::{BatchResult, admitted_batch::AdmittedBatchEntity},
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_commit::precondition::Precondition;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::Utc;
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
        context: &ContextSpec,
    ) -> Result<BatchResult, EntityError> {
        let target = placement.target();
        // Clause 5.6.7.4 runs the behaviour of Create Entity for each item, and
        // clause 5.5.4 excepts only the fragments of a partial update or a
        // merge from its NGSI-LD Null prohibitions.
        let (prepared, mut errors) = self.prepare_admitted_chunk(chunk, context, BatchWrite::Create.null_sentinel_admission()).await;

        if prepared.is_empty() {
            return Ok(BatchResult { success: Vec::new(), errors });
        }

        let lock_ids: Vec<Identifier> = prepared.iter().map(|entity| entity.id.clone()).collect(); // clone: guards outlive the borrowed preparation view
        let _id_guards = self.acquire_id_locks(target, &lock_ids).await;

        let now = Utc::now();
        let mut seen: HashSet<Identifier> = HashSet::with_capacity(prepared.len());
        let mut writes = Vec::with_capacity(prepared.len());
        for entity in prepared {
            if seen.insert(entity.id.clone()) {
                match self.chunk_insert(target, entity, Precondition::MustNotExist, now) {
                    Ok(write) => writes.push(write),
                    Err(failure) => errors.push(*failure),
                }
            } else {
                errors.push((entity.id.clone(), EntityError::already_exists(&entity.id))); // clone: error result owns the entity ID
            }
        }

        let outcome = self.commit_chunk(placement, writes).await?.settled();
        errors.extend(outcome.errors);

        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}
