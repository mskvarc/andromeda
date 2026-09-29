//! The store the entity service writes to and reads from.

use crate::{entity_snapshot::EntitySnapshot, error::EntityError, port::reader::EntityReader};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    precondition::Precondition,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex};

/// An in-memory store holding each entity's expanded payload and version.
#[derive(Default)]
pub(super) struct InMemoryStore {
    /// The stored entities, by identifier.
    entities: Mutex<HashMap<Identifier, (Value, DateTime<Utc>)>>,
}

impl InMemoryStore {
    /// Apply one write the way the store's atomic commit does.
    fn apply(&self, write: AtomicWrite) -> WriteOutcome {
        let mut entities = self.entities.lock().expect("the store lock is not poisoned");
        let id = (*write.entity_id).clone(); // clone: the store keys its rows by an owned identifier
        let current = entities.get(&id).map(|(_, modified_at)| *modified_at);
        match (write.entity, current) {
            (EntityMutation::Insert(_), Some(_)) => WriteOutcome::AlreadyExists,
            (EntityMutation::Insert(record), None) => {
                entities.insert(id, ((*record.payload).clone(), Utc::now())); // clone: the store owns its copy of the shared payload
                WriteOutcome::Created
            }
            (EntityMutation::Update(_) | EntityMutation::Delete, None) => WriteOutcome::NotFound,
            (EntityMutation::Update(record), Some(modified_at)) => match write.precondition {
                Precondition::UnchangedSince(expected) if expected != modified_at => WriteOutcome::Superseded,
                Precondition::UnchangedSince(_) | Precondition::None | Precondition::MustExist | Precondition::MustNotExist => {
                    entities.insert(id, ((*record.payload).clone(), Utc::now())); // clone: the store owns its copy of the shared payload
                    WriteOutcome::Updated
                }
            },
            (EntityMutation::Delete, Some(modified_at)) => {
                let payload = entities.remove(&id).map(|(payload, _)| payload).unwrap_or_default();
                WriteOutcome::Deleted(DeletedEntity {
                    id,
                    payload,
                    created_at: modified_at,
                    modified_at,
                    expires_at: None,
                })
            }
            (EntityMutation::None, _) => WriteOutcome::NoEntityChange,
        }
    }

    fn stored(&self, id: &Identifier) -> Option<(Value, DateTime<Utc>)> {
        self.entities.lock().expect("the store lock is not poisoned").get(id).cloned() // clone: the caller owns its copy of the row
    }
}

impl ContextCommitter for InMemoryStore {
    fn commit(&self, _target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let outcomes = writes.into_iter().map(|write| self.apply(write)).collect();
        Box::pin(async move { Ok(outcomes) })
    }

    fn commit_replayed(&self, target: &ContextTarget, _replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.commit(target, writes)
    }
}

#[async_trait]
impl EntityReader for InMemoryStore {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Value>, EntityError> {
        Ok(self.stored(id).map(|(payload, _)| payload))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, Value>, EntityError> {
        // clone: the answer owns the identifiers it is keyed by
        Ok(ids.iter().filter_map(|id| self.stored(id).map(|(payload, _)| (id.clone(), payload))).collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(self.stored(id).map(|(payload, modified_at)| EntitySnapshot { payload, modified_at }))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(ids
            .iter()
            // clone: the answer owns the identifiers it is keyed by
            .filter_map(|id| {
                self.stored(id)
                    .map(|(payload, modified_at)| (id.clone(), EntitySnapshot { payload, modified_at }))
            })
            .collect())
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(None)
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(HashMap::new())
    }
}
