//! The rows the in-memory store holds, and the rules a commit applies to
//! them, the way the PostgreSQL committer applies them.
//!
//! A write that states `UnchangedSince` lands only while the entity still
//! carries that version, a write that changes no current state included, and
//! every landed write of the current state gives the entity a new one.
//! The version is the row's key and write count: a row is keyed once and
//! counts every write, and a deleted entity created anew is keyed again. A
//! committed write stamps its `modifiedAt` with the instant it was accepted
//! at, so two writes accepted at the same instant leave the same `modifiedAt`
//! and still two versions.
//!
//! A deletion leaves the entity's Temporal Evolution standing at the
//! deletion's instant, as clause 5.6.6 has it. A write stamped before the
//! latest instant its entity holds, its `modifiedAt` or the instant its
//! Evolution was left at, is refused with that instant (clause 4.8).

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    entity_version::EntityVersion,
    precondition::Precondition,
    store_instant::StoreInstant,
    write_outcome::WriteOutcome,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, sync::Arc};

/// One stored entity.
#[derive(Clone)]
pub(crate) struct StoredRow {
    /// The entity's state.
    pub(crate) state: Arc<EntityState>,
    /// The version the row stands at.
    pub(crate) version: EntityVersion,
    /// The clause 4.8 `modifiedAt` the last write stamped.
    pub(crate) modified_at: DateTime<Utc>,
}

/// The stored entities and the last row key handed out.
#[derive(Default)]
pub(crate) struct StoredRows {
    /// Each entity's row.
    entities: HashMap<Identifier, StoredRow>,
    /// The latest instant of each entity whose Temporal Evolution stands
    /// without a current state, as a deletion left it.
    evolutions: HashMap<Identifier, DateTime<Utc>>,
    /// The key the last new row was given.
    last_key: i64,
}

impl StoredRows {
    /// Store `state` as the entity `id` names, at a new version, modified now.
    pub(crate) fn put(&mut self, id: &Identifier, state: Arc<EntityState>) {
        self.write(id, state, SystemInstant::new(Utc::now()).get());
    }

    /// Take the current state of the entity `id` names away, leaving its
    /// Temporal Evolution standing at `at`, as a deletion another writer
    /// committed then leaves it.
    pub(crate) fn retire(&mut self, id: &Identifier, at: DateTime<Utc>) {
        self.entities.remove(id);
        self.evolutions.insert(id.clone(), at); // clone: the rows own their keys
    }

    /// Store `state` as the entity `id` names, modified at `modified_at`: a
    /// standing row counts one more write, a new one is keyed afresh.
    pub(crate) fn write(&mut self, id: &Identifier, state: Arc<EntityState>, modified_at: DateTime<Utc>) {
        let version = if let Some(row) = self.entities.get(id) {
            EntityVersion::new(row.version.entity_key(), row.version.write_count() + 1)
        } else {
            self.last_key += 1;
            EntityVersion::new(self.last_key, 1)
        };
        // clone: the rows own their keys
        self.entities.insert(id.clone(), StoredRow { state, version, modified_at });
    }

    /// Remove the entity `id` names.
    pub(crate) fn remove(&mut self, id: &Identifier) {
        self.entities.remove(id);
    }

    /// The state the entity `id` names, as stored.
    pub(crate) fn payload(&self, id: &Identifier) -> Option<&Arc<EntityState>> {
        self.entities.get(id).map(|row| &row.state)
    }

    /// The row of the entity `id` names.
    pub(crate) fn row(&self, id: &Identifier) -> Option<&StoredRow> {
        self.entities.get(id)
    }

    /// The latest instant of the Temporal Evolution the entity `id` names
    /// holds without a current state.
    pub(crate) fn evolution_at(&self, id: &Identifier) -> Option<DateTime<Utc>> {
        self.evolutions.get(id).copied()
    }

    /// The version the entity `id` names is stored at.
    pub(crate) fn version(&self, id: &Identifier) -> Option<EntityVersion> {
        self.entities.get(id).map(|row| row.version)
    }

    /// The clause 4.8 `modifiedAt` the entity `id` names was last stamped
    /// with.
    pub(crate) fn modified_at(&self, id: &Identifier) -> Option<DateTime<Utc>> {
        self.entities.get(id).map(|row| row.modified_at)
    }

    /// Apply one write accepted at `accepted_at` the way the store's atomic
    /// commit does.
    pub(crate) fn apply(&mut self, write: &AtomicWrite, accepted_at: DateTime<Utc>) -> WriteOutcome {
        let id = &*write.entity_id;
        let current = self.entities.get(id).map(|row| (row.version, row.modified_at));
        match (&write.entity, current, write.precondition) {
            (EntityMutation::Insert(_), Some(_), Precondition::None) => WriteOutcome::Skipped,
            (EntityMutation::Insert(_), Some(_), Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
                WriteOutcome::AlreadyExists
            }
            (EntityMutation::Insert(record), None, _) => match self.evolutions.get(id) {
                Some(latest) if *latest > accepted_at => WriteOutcome::Predated(SystemInstant::new(*latest)),
                Some(_) | None => {
                    self.evolutions.remove(id);
                    self.write(id, Arc::clone(&record.state), accepted_at); // clone: Arc ref-count bump; the rows share the written state
                    WriteOutcome::Created(StoreInstant::new(accepted_at))
                }
            },
            // An update or a deletion stating a version matches the row only
            // while it carries that version, and a missing row carries none.
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
            (EntityMutation::Update(_) | EntityMutation::Delete, Some((version, _)), Precondition::UnchangedSince(expected)) if expected != version => {
                WriteOutcome::Superseded
            }
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::None | Precondition::MustExist | Precondition::MustNotExist) => {
                WriteOutcome::NotFound
            }
            (EntityMutation::Update(_) | EntityMutation::Delete, Some((_, modified_at)), _) if modified_at > accepted_at => {
                WriteOutcome::Predated(SystemInstant::new(modified_at))
            }
            (EntityMutation::Update(record), Some(_), _) => {
                self.write(id, Arc::clone(&record.state), accepted_at); // clone: Arc ref-count bump; the rows share the written state
                WriteOutcome::Updated(StoreInstant::new(accepted_at))
            }
            (EntityMutation::Delete, Some((_, modified_at)), _) => {
                self.retire(id, accepted_at);
                WriteOutcome::Deleted(
                    DeletedEntity {
                        id: id.clone(), // clone: the outcome owns the identifier
                        created_at: modified_at,
                        modified_at,
                        expires_at: None,
                    },
                    StoreInstant::new(accepted_at),
                )
            }
            (EntityMutation::None, current, precondition) => precondition.untouched_outcome(current.map(|(version, _)| version)),
        }
    }
}
