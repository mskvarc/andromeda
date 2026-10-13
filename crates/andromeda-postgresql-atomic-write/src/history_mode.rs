//! Selection of native or deferred temporal history within the PostgreSQL anchor.
//!
//! Deferral exists for live-branch ingest throughput, where queuing an intent
//! is cheaper than writing the hypertable rows in the request's transaction
//! and a resident drain applies the queue behind it, and for a respond-async
//! commit, whose client asked to be answered before the consequences are
//! produced (RFC 7240 section 4.1): its history is queued in either append
//! mode. The store's temporal reads include queued intents, so a queued
//! history is answered at once either way.
//!
//! None of that holds for a Snapshot branch. Its history tables are plain
//! tables, it is bounded and read-mostly, and nothing drains its intent queue
//! in the background, so an intent queued there would stay invisible to the
//! branch's Temporal API until some later temporal mutation of the same entity
//! forced it through. A Snapshot branch therefore always records its history
//! in the write's own transaction.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::{config::TemporalAppendMode, port::temporal_intent_publisher::TemporalIntentPublisher};
use std::sync::Arc;

/// How one tenant's PostgreSQL anchor commits temporal append consequences.
pub struct TemporalHistoryCommitMode {
    /// Whether the live branch's history is recorded in the write's
    /// transaction or queued for the drain.
    append_mode: TemporalAppendMode,
    /// Transactional recorder history is written with, and queued intents are
    /// keyed through.
    recorder: Arc<TemporalRecorder>,
    /// Edge notification that reduces delivery latency without carrying work.
    publisher: Arc<dyn TemporalIntentPublisher>,
}

/// Whether a commit's client waits for its consequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitResponse {
    /// The client is answered once the commit's consequences are produced.
    Synchronous,
    /// The client asked to be answered before them (`Prefer: respond-async`).
    RespondAsync,
}

/// Where one commit's temporal history is written.
pub(crate) enum HistoryPlacement<'a> {
    /// Into the history tables, inside the commit's own transaction.
    Inline(&'a TemporalRecorder),
    /// Into the live branch's intent queue as the records `recorder` makes
    /// of them, for the deferred drain to apply, waking the drain through
    /// `publisher` once the commit lands. Only a live-branch commit is placed
    /// here.
    Queued {
        /// The recorder whose dictionaries key the queued entries.
        recorder: &'a TemporalRecorder,
        /// The drain's wake.
        publisher: &'a dyn TemporalIntentPublisher,
    },
}

impl TemporalHistoryCommitMode {
    /// The history commit mode of a tenant running `append_mode`.
    #[must_use]
    pub fn new(append_mode: TemporalAppendMode, recorder: Arc<TemporalRecorder>, publisher: Arc<dyn TemporalIntentPublisher>) -> TemporalHistoryCommitMode {
        TemporalHistoryCommitMode {
            append_mode,
            recorder,
            publisher,
        }
    }

    /// Where a commit against `target` answered as `response` writes its
    /// temporal history.
    pub(crate) fn placement(&self, target: &ContextTarget, response: CommitResponse) -> HistoryPlacement<'_> {
        match (target, self.append_mode, response) {
            (ContextTarget::Snapshot(_), _, _) | (ContextTarget::Live, TemporalAppendMode::Native, CommitResponse::Synchronous) => {
                HistoryPlacement::Inline(&self.recorder)
            }
            (ContextTarget::Live, TemporalAppendMode::Native, CommitResponse::RespondAsync)
            | (ContextTarget::Live, TemporalAppendMode::Deferred, CommitResponse::Synchronous | CommitResponse::RespondAsync) => HistoryPlacement::Queued {
                recorder: &self.recorder,
                publisher: self.publisher.as_ref(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::history_mode::{CommitResponse, HistoryPlacement, TemporalHistoryCommitMode};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_cache_config::NameCacheConfig};
    use andromeda_postgresql_temporal_store::{columnstore_capability::ColumnstoreSupport, recorder::TemporalRecorder, repo::TemporalRepository};
    use andromeda_temporal::{config::TemporalAppendMode, port::temporal_intent_publisher::NoTemporalIntentPublisher};
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    /// The commit mode of `append_mode` over a pool that never connects: the
    /// placement decision only names its recorder.
    fn mode(append_mode: TemporalAppendMode) -> TemporalHistoryCommitMode {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused@127.0.0.1:1/unused")
            .expect("a lazy pool is built without connecting");
        let recorder = Arc::new(TemporalRecorder::new(Arc::new(TemporalRepository::new(
            &pool,
            Arc::new(NameCache::<AttributeName>::new(&pool, &NameCacheConfig::default())),
            Arc::new(NameCache::<EntityTypeName>::new(&pool, &NameCacheConfig::default())),
            ColumnstoreSupport::Absent,
        ))));
        TemporalHistoryCommitMode::new(append_mode, recorder, Arc::new(NoTemporalIntentPublisher))
    }

    fn snapshot() -> ContextTarget {
        ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(4).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:history".parse().expect("a valid snapshot URI"),
        ))
    }

    #[tokio::test]
    async fn deferred_mode_queues_the_live_branchs_history() {
        assert!(matches!(
            mode(TemporalAppendMode::Deferred).placement(&ContextTarget::Live, CommitResponse::Synchronous),
            HistoryPlacement::Queued { .. }
        ));
    }

    /// Clause 5.5.15 permits the Temporal API on a Snapshot, so its history
    /// must be readable there as soon as the write answers; nothing drains a
    /// Snapshot branch's intent queue behind it.
    #[tokio::test]
    async fn every_mode_records_a_snapshot_branchs_history_in_the_write_transaction() {
        for append_mode in [TemporalAppendMode::Native, TemporalAppendMode::Deferred] {
            for response in [CommitResponse::Synchronous, CommitResponse::RespondAsync] {
                assert!(matches!(mode(append_mode).placement(&snapshot(), response), HistoryPlacement::Inline(_)));
            }
        }
    }

    #[tokio::test]
    async fn native_mode_records_a_synchronous_commits_history_in_the_write_transaction() {
        assert!(matches!(
            mode(TemporalAppendMode::Native).placement(&ContextTarget::Live, CommitResponse::Synchronous),
            HistoryPlacement::Inline(_)
        ));
    }

    /// RFC 7240 section 4.1 lets a respond-async answer precede the
    /// processing, and the store's reads include queued intents, so the
    /// history is queued and still answered at once.
    #[tokio::test]
    async fn a_respond_async_commit_queues_its_history_as_intents_in_native_mode() {
        assert!(matches!(
            mode(TemporalAppendMode::Native).placement(&ContextTarget::Live, CommitResponse::RespondAsync),
            HistoryPlacement::Queued { .. }
        ));
    }

    #[tokio::test]
    async fn a_respond_async_commit_queues_its_history_as_intents_in_deferred_mode() {
        assert!(matches!(
            mode(TemporalAppendMode::Deferred).placement(&ContextTarget::Live, CommitResponse::RespondAsync),
            HistoryPlacement::Queued { .. }
        ));
    }
}
