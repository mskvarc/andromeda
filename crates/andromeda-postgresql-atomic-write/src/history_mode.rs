//! Placement of one commit's temporal history within the PostgreSQL anchor.
//!
//! A respond-async commit, whose client asked to be answered before the
//! consequences are produced (RFC 7240 section 4.1), queues its live-branch
//! history as intents for the resident drain to apply behind it. A synchronous
//! live commit records the history of its Entities in its own transaction,
//! except for the Entities whose earlier history is still queued: clauses
//! 4.5.7, 4.5.8 and 4.8 read an Entity's history in commit order, so their
//! intents queue behind the earlier ones. The store's temporal reads include
//! queued intents, so a queued history is answered at once.
//!
//! None of that holds for a Snapshot branch. Its history tables are plain
//! tables, it is bounded and read-mostly, and nothing drains its intent queue
//! in the background, so an intent queued there would stay invisible to the
//! branch's Temporal API until some later temporal mutation of the same entity
//! forced it through. A Snapshot branch therefore always records its history
//! in the write's own transaction.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::port::temporal_intent_publisher::TemporalIntentPublisher;
use std::sync::Arc;

/// How one tenant's PostgreSQL anchor commits temporal append consequences.
pub struct TemporalHistoryCommitMode {
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
    /// of them, for the drain to apply, waking the drain through
    /// `publisher` once the commit lands. Only a live-branch commit is placed
    /// here.
    Queued {
        /// The recorder whose dictionaries key the queued entries.
        recorder: &'a TemporalRecorder,
        /// The drain's wake.
        publisher: &'a dyn TemporalIntentPublisher,
    },
    /// Into the history tables inside the commit's own transaction for every
    /// Entity with no queued history, and into the live branch's intent queue
    /// behind the earlier intents of every other, waking the drain through
    /// `publisher` once the commit lands when anything was queued. Only a
    /// live-branch commit is placed here.
    BehindQueued {
        /// The recorder history is written with and queued entries are keyed
        /// through.
        recorder: &'a TemporalRecorder,
        /// The drain's wake.
        publisher: &'a dyn TemporalIntentPublisher,
    },
}

impl TemporalHistoryCommitMode {
    /// The history commit mode of a tenant recording through `recorder` and
    /// waking its drain through `publisher`.
    #[must_use]
    pub fn new(recorder: Arc<TemporalRecorder>, publisher: Arc<dyn TemporalIntentPublisher>) -> TemporalHistoryCommitMode {
        TemporalHistoryCommitMode { recorder, publisher }
    }

    /// Where a commit against `target` answered as `response` writes its
    /// temporal history.
    pub(crate) fn placement(&self, target: &ContextTarget, response: CommitResponse) -> HistoryPlacement<'_> {
        match (target, response) {
            (ContextTarget::Snapshot(_), CommitResponse::Synchronous | CommitResponse::RespondAsync) => HistoryPlacement::Inline(&self.recorder),
            (ContextTarget::Live, CommitResponse::Synchronous) => HistoryPlacement::BehindQueued {
                recorder: &self.recorder,
                publisher: self.publisher.as_ref(),
            },
            (ContextTarget::Live, CommitResponse::RespondAsync) => HistoryPlacement::Queued {
                recorder: &self.recorder,
                publisher: self.publisher.as_ref(),
            },
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_mode::{CommitResponse, HistoryPlacement, TemporalHistoryCommitMode};
    use andromeda_configuration::schema::{name_cache::NameCacheConfig, write_reserve::WriteReserve};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_name_dictionary::name_cache::NameCache;
    use andromeda_postgresql_pool::read_share::ReadShare;
    use andromeda_postgresql_temporal_store::{columnstore_capability::ColumnstoreSupport, recorder::TemporalRecorder, repo::TemporalRepository};
    use andromeda_temporal::port::temporal_intent_publisher::NoTemporalIntentPublisher;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    /// A commit mode over a pool that never connects: the placement decision
    /// only names its recorder.
    fn mode() -> TemporalHistoryCommitMode {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused@127.0.0.1:1/unused")
            .expect("a lazy pool is built without connecting");
        let recorder = Arc::new(TemporalRecorder::new(Arc::new(TemporalRepository::new(
            &pool,
            ReadShare::of(&pool, WriteReserve::DEFAULT),
            Arc::new(NameCache::<AttributeName>::new(&pool, &NameCacheConfig::default())),
            Arc::new(NameCache::<EntityTypeName>::new(&pool, &NameCacheConfig::default())),
            ColumnstoreSupport::Absent,
        ))));
        TemporalHistoryCommitMode::new(recorder, Arc::new(NoTemporalIntentPublisher))
    }

    fn snapshot() -> ContextTarget {
        ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(4).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:history".parse().expect("a valid snapshot URI"),
        ))
    }

    /// Clause 5.5.15 permits the Temporal API on a Snapshot, so its history
    /// must be readable there as soon as the write answers; nothing drains a
    /// Snapshot branch's intent queue behind it.
    #[tokio::test]
    async fn a_snapshot_branchs_history_is_recorded_in_the_write_transaction_for_either_response() {
        for response in [CommitResponse::Synchronous, CommitResponse::RespondAsync] {
            assert!(matches!(mode().placement(&snapshot(), response), HistoryPlacement::Inline(_)));
        }
    }

    /// Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in commit
    /// order, so a synchronous commit records inline only what no queued
    /// history of its Entity precedes.
    #[tokio::test]
    async fn a_live_synchronous_commits_history_queues_behind_its_entities_queued_history() {
        assert!(matches!(
            mode().placement(&ContextTarget::Live, CommitResponse::Synchronous),
            HistoryPlacement::BehindQueued { .. }
        ));
    }

    /// RFC 7240 section 4.1 lets a respond-async answer precede the
    /// processing, and the store's reads include queued intents, so the
    /// history is queued and still answered at once.
    #[tokio::test]
    async fn a_live_respond_async_commit_queues_its_history_as_intents() {
        assert!(matches!(
            mode().placement(&ContextTarget::Live, CommitResponse::RespondAsync),
            HistoryPlacement::Queued { .. }
        ));
    }
}
