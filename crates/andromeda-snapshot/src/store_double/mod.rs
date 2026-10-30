//! An in-memory [`SnapshotStore`] double for the policy tests.
//!
//! It holds a few Snapshots, answers what scoping reads of them, hands out
//! preparation jobs the way the store does -- a clone only once its source has
//! left `preparing` -- and records what preparations do to their claims.
//! Every other operation fails loudly: no test exercises it, and a silent
//! default would let a test pass for the wrong reason.

/// Where one stored Snapshot's preparation stands.
mod entry;
/// The double's answers to the snapshot store port.
mod port;

use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    snapshot_invariant::SnapshotInvariant,
    store::{InsertSnapshotParams, SnapshotPreparationJob},
    store_double::entry::{Entry, Stage},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus};
use std::{
    mem::take,
    sync::{
        Arc,
        Mutex,
        MutexGuard,
        atomic::{AtomicU32, Ordering},
    },
};
use tokio::sync::Notify;

/// Whether the worker still holds the preparation claim when it restarts it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaimHold {
    /// The claim is this worker's for its whole preparation.
    Held,
    /// Another worker took the claim over.
    Lost,
}

/// How the double answers a preparation lease renewal.
#[derive(Clone, Debug)]
pub(crate) enum Renewal {
    /// The renewal extends a claim this worker still holds.
    Retained,
    /// The renewal waits until the gate is notified, as an UPDATE waits on a
    /// row lock, and then extends the claim.
    GatedOn(Arc<Notify>),
    /// The renewal finds the claim lost.
    Lost,
    /// The renewal fails: the store is saturated.
    Fails,
}

/// A terminal status a preparation recorded, with its execution details.
#[derive(Debug)]
pub(crate) struct RecordedStatus {
    /// The Snapshot it was recorded for.
    pub(crate) id: Identifier,
    /// The status recorded.
    pub(crate) status: SnapshotStatus,
    /// The entity query execution details recorded with it.
    pub(crate) queries_details: Option<Vec<ExecutionResultDetails>>,
    /// The temporal query execution details recorded with it.
    pub(crate) temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
}

/// One clone copy: the clone, and the status its source had when it was copied.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CloneCopy {
    /// The clone the source was copied into.
    pub(crate) target: Identifier,
    /// The source's status at the time of the copy.
    pub(crate) source_status: SnapshotStatus,
}

/// A store holding a few Snapshots whose branches are in one condition.
pub(crate) struct StoreDouble {
    entries: Mutex<Vec<Entry>>,
    condition: BranchCondition,
    claim: ClaimHold,
    restarts: AtomicU32,
    renewal: Option<Renewal>,
    renewals: AtomicU32,
    recorded: Mutex<Vec<RecordedStatus>>,
    copies: Mutex<Vec<CloneCopy>>,
}

impl StoreDouble {
    /// A store holding `snapshot`, its branch in `condition`, and its
    /// preparation claim held by the worker as `claim` says.
    pub(crate) fn new(snapshot: Snapshot, condition: BranchCondition, claim: ClaimHold) -> StoreDouble {
        StoreDouble {
            entries: Mutex::new(vec![Entry::new(snapshot, None)]),
            condition,
            claim,
            restarts: AtomicU32::new(0),
            renewal: None,
            renewals: AtomicU32::new(0),
            recorded: Mutex::new(Vec::new()),
            copies: Mutex::new(Vec::new()),
        }
    }

    /// Answer every lease renewal as `renewal` says; without it a renewal
    /// fails loudly, since no test expects one.
    pub(crate) fn with_renewal(self, renewal: Renewal) -> StoreDouble {
        StoreDouble {
            renewal: Some(renewal),
            ..self
        }
    }

    /// Have another worker hold the waiting Snapshot `id`'s claim.
    pub(crate) fn hold_elsewhere(&self, id: &Identifier) {
        self.move_stage(id, Stage::Waiting, Stage::Claimed);
    }

    /// Let the claim another worker holds on `id` lapse unfinished, as its
    /// lease expiring does.
    pub(crate) fn lapse_hold(&self, id: &Identifier) {
        self.move_stage(id, Stage::Claimed, Stage::Waiting);
    }

    /// Move the Snapshot `id` from stage `from` to stage `to`.
    fn move_stage(&self, id: &Identifier, from: Stage, to: Stage) {
        let mut entries = self.entries();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.snapshot.id == *id && entry.stage == from)
            .expect("the double holds the Snapshot in the expected stage");
        entry.stage = to;
    }

    /// How often a preparation lease renewal was asked for.
    pub(crate) fn renewals(&self) -> u32 {
        self.renewals.load(Ordering::SeqCst)
    }

    /// Also hold `clone`, a clone job of `source`.
    pub(crate) fn with_clone(self, clone: Snapshot, source: &Identifier) -> StoreDouble {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(Entry::new(clone, Some(source.clone()))); // clone: the stored job owns its source identifier.
        }
        self
    }

    /// Also hold `snapshot`, a capture job.
    pub(crate) fn with_capture(self, snapshot: Snapshot) -> StoreDouble {
        self.entries().push(Entry::new(snapshot, None));
        self
    }

    /// How often a preparation claim was restarted.
    pub(crate) fn restarts(&self) -> u32 {
        self.restarts.load(Ordering::SeqCst)
    }

    /// The terminal statuses recorded so far, in order.
    pub(crate) fn take_recorded(&self) -> Vec<RecordedStatus> {
        self.recorded.lock().map(|mut recorded| take(&mut *recorded)).unwrap_or_default()
    }

    /// The clone copies made so far, in order.
    pub(crate) fn take_copies(&self) -> Vec<CloneCopy> {
        self.copies.lock().map(|mut copies| take(&mut *copies)).unwrap_or_default()
    }

    /// The stored Snapshots.
    /// Hold the Snapshot a create inserts, as the record a later read returns.
    fn insert(&self, params: &InsertSnapshotParams<'_>) {
        let priority = u8::try_from(params.snapshot_priority)
            .ok()
            .and_then(|priority| SnapshotPriority::try_from(priority).ok())
            .expect("the service inserts a priority within clause 5.2.41");
        let snapshot = Snapshot::builder()
            .id(params.id.clone()) // clone: the stored record owns its identifier.
            .snapshot_priority(priority)
            .status(params.status)
            .created_at(params.created_at)
            .modified_at(params.modified_at)
            .expires_at(params.expires_at)
            .last_used_at(params.last_used_at)
            .build();
        self.entries().push(Entry::new(snapshot, params.clone_source_id.cloned()));
    }

    fn entries(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().expect("the store double lock is not poisoned")
    }

    /// Move a preparation this worker holds out of `preparing`, recording the
    /// status; 0 when the Snapshot is gone, not preparing, or the claim lost.
    fn end_preparation(
        &self,
        id: &Identifier,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> u64 {
        if self.claim == ClaimHold::Lost {
            return 0;
        }
        let mut entries = self.entries();
        let Some(entry) = entries.iter_mut().find(|entry| entry.snapshot.id == *id && entry.stage == Stage::Claimed) else {
            return 0;
        };
        entry.stage = Stage::Ended(status);
        self.recorded.lock().expect("the store double lock is not poisoned").push(RecordedStatus {
            id: id.clone(), // clone: the record owns the identifier it names.
            status,
            queries_details: queries_details.map(<[ExecutionResultDetails]>::to_vec),
            temporal_queries_details: temporal_queries_details.map(<[ExecutionResultDetails]>::to_vec),
        });
        1
    }

    /// Claim the first waiting Snapshot whose clone source, if any, has left
    /// `preparing` or is gone -- the store's claim predicate.
    fn claim_next(&self) -> Option<SnapshotPreparationJob> {
        self.claim_first(|_| true)
    }

    /// Claim the created Snapshot `id` under the same predicate.
    fn claim_created(&self, id: &Identifier) -> Option<SnapshotPreparationJob> {
        self.claim_first(|entry| entry.snapshot.id == *id)
    }

    /// Claim the first claimable Snapshot `wanted` accepts.
    fn claim_first(&self, wanted: impl Fn(&Entry) -> bool) -> Option<SnapshotPreparationJob> {
        let mut entries = self.entries();
        let source_preparing = |source: &Identifier, entries: &[Entry]| {
            entries
                .iter()
                .any(|entry| entry.snapshot.id == *source && entry.status() == SnapshotStatus::Preparing)
        };
        let position = entries.iter().position(|entry| {
            wanted(entry) && entry.stage == Stage::Waiting && entry.clone_source.as_ref().is_none_or(|source| !source_preparing(source, &entries))
        })?;
        let entry = &mut entries[position];
        entry.stage = Stage::Claimed;
        Some(SnapshotPreparationJob {
            snapshot: entry.snapshot.clone(),            // clone: the job owns its Snapshot.
            clone_source_id: entry.clone_source.clone(), // clone: the job owns its source identifier.
            query_context: JsonLiteral::null(),
        })
    }

    /// Copy `source` into the claimed clone `target` and end it `success`, or
    /// refuse as the store does when the source is gone.
    fn copy_clone(&self, source: &Identifier, target: &Identifier) -> Result<(), SnapshotError> {
        let source_status = self
            .entries()
            .iter()
            .find(|entry| entry.snapshot.id == *source)
            .map(Entry::status)
            .ok_or_else(|| SnapshotInvariant::CloneSourceMissing {
                source_snapshot: source.clone(),
            })?; // clone: the error owns the Snapshot it names.
        self.copies.lock().expect("the store double lock is not poisoned").push(CloneCopy {
            target: target.clone(), // clone: the record owns the identifier it names.
            source_status,
        });
        match self.end_preparation(target, SnapshotStatus::Success, None, None) {
            0 => Err(SnapshotInvariant::ClaimLost { snapshot: target.clone() }.into()), // clone: the error owns the Snapshot it names.
            _ => Ok(()),
        }
    }
}
