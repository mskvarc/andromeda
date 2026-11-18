//! Counting this broker process's own live writes toward a background spatial
//! catch-up.
//!
//! Under `watermark` consistency the index is caught up from the store, and a
//! geo query takes in only a bounded number of changes itself. Counted around
//! the commit port every current-state write goes through, a load this process
//! writes wakes the background catch-up while it runs, so the index keeps pace
//! with it and a query after the load finds little to read. The count is a
//! latency hint only. A write this process does not count, or another
//! process's write, is still taken in by the next catch-up.

use crate::spatial_watermark::SpatialWatermark;
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use std::sync::Arc;

/// A commit port that counts every live write it forwards that landed toward
/// the next background spatial catch-up.
pub struct SpatialCatchUpCommitter {
    /// The commit port every write is forwarded to.
    inner: Arc<dyn ContextCommitter>,
    /// The watermark the writes are counted toward.
    watermark: Arc<SpatialWatermark>,
}

impl SpatialCatchUpCommitter {
    /// Count every live write forwarded to `inner` that landed toward
    /// `watermark`'s next background catch-up.
    #[must_use]
    pub fn new(inner: Arc<dyn ContextCommitter>, watermark: Arc<SpatialWatermark>) -> SpatialCatchUpCommitter {
        SpatialCatchUpCommitter { inner, watermark }
    }

    /// Count the writes of one commit that may have changed an entity's
    /// location or types. A Snapshot branch is never indexed.
    fn count(&self, target: &ContextTarget, outcomes: &[WriteOutcome]) {
        if target.snapshot().is_some() {
            return;
        }
        self.watermark
            .writes_committed(outcomes.iter().filter(|outcome| restates_an_entity(outcome)).count());
    }
}

/// Whether a write that ended with `outcome` wrote an entity's current state.
///
/// A deletion removes the search row a catch-up reads, so the index keeps the
/// entity as a stale candidate until the next rebuild whether or not it is
/// counted.
const fn restates_an_entity(outcome: &WriteOutcome) -> bool {
    match outcome {
        WriteOutcome::Created(_) | WriteOutcome::Updated(_) => true,
        WriteOutcome::Deleted(..)
        | WriteOutcome::AlreadyExists
        | WriteOutcome::NotFound
        | WriteOutcome::Superseded
        | WriteOutcome::Skipped
        | WriteOutcome::NoEntityChange
        | WriteOutcome::Predated(_)
        | WriteOutcome::Refused(_) => false,
    }
}

impl ContextCommitter for SpatialCatchUpCommitter {
    fn commit(&self, target: &ContextTarget, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone(); // clone: the owned future outlives the borrowed target
        Box::pin(async move {
            let outcomes = self.inner.commit(&target, accepted_at, writes).await?;
            self.count(&target, &outcomes);
            Ok(outcomes)
        })
    }

    fn commit_enqueued(&self, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async move {
            let outcomes = self.inner.commit_enqueued(accepted_at, writes).await?;
            self.count(&ContextTarget::Live, &outcomes);
            Ok(outcomes)
        })
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone(); // clone: the owned future outlives the borrowed target
        Box::pin(async move {
            let outcomes = self.inner.commit_replayed(&target, replay, writes).await?;
            self.count(&target, &outcomes);
            Ok(outcomes)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        port::{blocking_executor::BlockingExecutor, spatial_catch_up_signal::SpatialCatchUpSignal, spatial_snapshot::EntitySpatialSnapshotSource},
        recording_ports::{InlineBlockingExecutor, RecordingSignal, Script, ScriptedCommitter},
        spatial_catch_up_committer::SpatialCatchUpCommitter,
        spatial_watermark::SpatialWatermark,
        spatial_write_visibility::{
            geo_query::{placement, uri},
            replicated_store::ReplicatedStore,
        },
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        committer::ContextCommitter,
        deleted_entity::DeletedEntity,
        entity_mutation::EntityMutation,
        store_instant::StoreInstant,
        submission_refusal::SubmissionRefusal,
        write_outcome::WriteOutcome,
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use andromeda_spatial_index::entity::index::SpatialEntityIndex;
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    /// A committer answering by `script`, counting toward a watermark whose
    /// wakes `signal` records.
    fn counting(script: Script) -> (SpatialCatchUpCommitter, Arc<RecordingSignal>) {
        let signal = Arc::new(RecordingSignal::default());
        let watermark = Arc::new(SpatialWatermark::new(
            Arc::new(SpatialEntityIndex::new()),
            Arc::new(ReplicatedStore::default()) as Arc<dyn EntitySpatialSnapshotSource>,
            placement(),
            Arc::new(InlineBlockingExecutor) as Arc<dyn BlockingExecutor>,
            Arc::clone(&signal) as Arc<dyn SpatialCatchUpSignal>,
        ));
        (SpatialCatchUpCommitter::new(Arc::new(ScriptedCommitter(script)), watermark), signal)
    }

    /// One thousand writes, enough for one wake when every one counts.
    fn thousand_writes() -> Vec<AtomicWrite> {
        (0..1_000)
            .map(|n| {
                AtomicWrite::builder()
                    .entity_id(Arc::new(uri(&format!("urn:ngsi-ld:Vehicle:{n}"))))
                    .entity(EntityMutation::None)
                    .build()
            })
            .collect()
    }

    fn accepted() -> SystemInstant {
        SystemInstant::new(Utc::now())
    }

    #[tokio::test]
    async fn a_landed_live_write_counts_toward_a_catch_up() {
        for landed in [(|| WriteOutcome::Created(StoreInstant::new(Utc::now()))) as fn() -> WriteOutcome, || {
            WriteOutcome::Updated(StoreInstant::new(Utc::now()))
        }] {
            let (committer, signal) = counting(Script::Answer(landed));
            committer
                .commit(&ContextTarget::Live, accepted(), thousand_writes())
                .await
                .expect("the writes land");
            assert_eq!(signal.wakes(), 1);
        }
    }

    #[tokio::test]
    async fn a_respond_async_write_counts_toward_a_catch_up() {
        let (committer, signal) = counting(Script::Answer(|| WriteOutcome::Updated(StoreInstant::new(Utc::now()))));
        committer.commit_enqueued(accepted(), thousand_writes()).await.expect("the writes land");
        assert_eq!(signal.wakes(), 1);
    }

    #[tokio::test]
    async fn a_snapshot_branch_write_counts_nothing() {
        let (committer, signal) = counting(Script::Answer(|| WriteOutcome::Updated(StoreInstant::new(Utc::now()))));
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(3).expect("positive key"),
            "urn:ngsi-ld:Snapshot:a".parse().expect("a well-formed URI"),
        ));
        committer.commit(&snapshot, accepted(), thousand_writes()).await.expect("the writes land");
        assert_eq!(signal.wakes(), 0);
    }

    #[tokio::test]
    async fn a_write_that_wrote_no_current_state_counts_nothing() {
        for unwritten in [
            (|| WriteOutcome::AlreadyExists) as fn() -> WriteOutcome,
            || WriteOutcome::NotFound,
            || WriteOutcome::Superseded,
            || WriteOutcome::Skipped,
            || WriteOutcome::NoEntityChange,
            || WriteOutcome::Predated(SystemInstant::new(Utc::now())),
            || WriteOutcome::Refused(SubmissionRefusal::RelationshipMissingObject),
            || {
                WriteOutcome::Deleted(
                    DeletedEntity {
                        id: uri("urn:ngsi-ld:Vehicle:gone"),
                        created_at: Utc::now(),
                        modified_at: Utc::now(),
                        expires_at: None,
                    },
                    StoreInstant::new(Utc::now()),
                )
            },
        ] {
            let (committer, signal) = counting(Script::Answer(unwritten));
            committer
                .commit(&ContextTarget::Live, accepted(), thousand_writes())
                .await
                .expect("the commit answers");
            assert_eq!(signal.wakes(), 0, "{:?}", unwritten());
        }
    }

    #[tokio::test]
    async fn a_failed_commit_reports_its_error_and_counts_nothing() {
        let (committer, signal) = counting(Script::Fail);
        assert!(committer.commit(&ContextTarget::Live, accepted(), thousand_writes()).await.is_err());
        assert_eq!(signal.wakes(), 0);
    }
}
