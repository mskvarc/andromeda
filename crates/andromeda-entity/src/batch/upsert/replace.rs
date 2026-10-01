//! Batch upsert in replace mode (clause 5.6.8.4): an Entity that exists is
//! replaced, one that does not is created.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        UpsertPolicy,
        admitted_batch::AdmittedBatchEntity,
        chunk::{ChunkWrite, UpdatedPayloads},
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
        upsert::{UpsertOutcome, stored_payload},
    },
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    notification_plan::modification,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{mutation::selection::OverwritePolicy, written_observation::AddressedInstances};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// What a replacement is built from on each attempt.
enum Replacement {
    /// The request's prepared payload, which no attempt has spent yet.
    Requested(Box<PreparedEntity>),
    /// The replacement an earlier attempt built, kept so a later attempt does
    /// not expand the request again.
    Built {
        /// The entity the replacement is for.
        id: Identifier,
        /// The replacement payload.
        payload: Arc<Value>,
    },
}

/// Replacing each entity of a chunk, or creating it when an attempt finds it
/// absent (clause 5.6.8.4, replace mode).
struct ReplaceRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
}

impl ChunkRewrite for ReplaceRewrite<'_> {
    type Entry = Replacement;
    type Reads = ();

    fn id(entry: &Replacement) -> &Identifier {
        match entry {
            Replacement::Requested(prepared) => &prepared.id,
            Replacement::Built { id, .. } => id,
        }
    }

    fn build(
        &self,
        entry: Replacement,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: DateTime<Utc>,
    ) -> Result<PlannedWrite<Replacement>, Box<(Identifier, EntityError)>> {
        let (target, service) = (self.target, self.service);
        let (write, payload) = match (entry, snapshot) {
            (Replacement::Requested(prepared), Some(snapshot)) => {
                let PreparedEntity { id, expanded, .. } = *prepared;
                let payload = Arc::new(expanded);
                (service.replacement_write(target, id, &payload, &snapshot.payload, now)?, payload)
            }
            (Replacement::Built { id, payload }, Some(snapshot)) => (service.replacement_write(target, id, &payload, &snapshot.payload, now)?, payload),
            (Replacement::Requested(prepared), None) => {
                let write = service.chunk_insert(target, *prepared, now)?;
                let payload = stored_payload(&write)?;
                (write, payload)
            }
            (Replacement::Built { id, payload }, None) => {
                let write = service.chunk_insert_payload(target, id, payload, now)?;
                let payload = stored_payload(&write)?;
                (write, payload)
            }
        };
        Ok(PlannedWrite {
            entry: Replacement::Built {
                id: write.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
                payload,
            },
            write,
            alongside: None,
        })
    }
}

impl EntityService {
    pub(in crate::batch) async fn batch_upsert_replace_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        errors: &mut Vec<(Identifier, EntityError)>,
    ) -> Result<UpsertOutcome, EntityError> {
        // Clause 5.6.8.4 gives replace mode two branches and neither admits the
        // sentinel: an Entity that does not exist is created by "the behaviour
        // defined by clause 5.6.1", and one that does "shall be completely
        // replaced by the new Entity content provided", which is the Replace
        // Entity of clause 5.6.18 -- whose clause 5.6.18.4 states outright that
        // NGSI-LD Nulls are not supported. Clause 5.5.4 excepts only partial
        // update and merge fragments, and a replacement is neither.
        let (prepared, mut chunk_errors) = self
            .prepare_admitted_chunk(chunk, BatchWrite::Upsert(UpsertPolicy::Replace).null_sentinel_admission())
            .await;
        errors.append(&mut chunk_errors);
        let rewrite = ReplaceRewrite {
            service: self,
            target: placement.target(),
        };
        let entries = prepared.into_iter().map(|prepared| Replacement::Requested(Box::new(prepared))).collect();
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, entries).await?;
        errors.extend(outcome.errors);
        Ok((
            BatchResult {
                success: outcome.success,
                errors: Vec::new(),
            },
            outcome.created,
        ))
    }

    /// The write that replaces `previous` with `payload`, tombstoning whatever
    /// the replacement drops (NGSI-LD 5.6.8).
    ///
    /// Mirrors the single replace (clause 5.6.18): one precise event for every
    /// created, updated, or deleted attribute instance.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure, boxed because a
    /// failure is the rare outcome, for the batch's per-entity report, when either state's members cannot be read.
    fn replacement_write(
        &self,
        target: &ContextTarget,
        id: Identifier,
        payload: &Arc<Value>,
        previous: &Arc<Value>,
        now: DateTime<Utc>,
    ) -> Result<ChunkWrite, Box<(Identifier, EntityError)>> {
        let change = match modification(previous, payload) {
            Ok(change) => change,
            Err(error) => return Err(Box::new((id, error))),
        };
        self.chunk_update(
            target,
            id,
            UpdatedPayloads {
                previous,
                merged: Arc::clone(payload), // clone: Arc ref-count bump; the payload is reused on a retry.
                addressed: AddressedInstances::EntityFragment {
                    fragment: payload,
                    overwrite: OverwritePolicy::Overwrite,
                },
            },
            change,
            now,
        )
    }
}
