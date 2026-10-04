//! Selection of native or deferred temporal history within the PostgreSQL anchor.
//!
//! Deferral exists for one reason: live-branch ingest throughput, where queuing
//! an intent is cheaper than writing the hypertable rows in the request's
//! transaction and a resident drain applies the queue behind it. None of that
//! holds for a Snapshot branch. Its history tables are plain tables, it is
//! bounded and read-mostly, and nothing drains its intent queue in the
//! background, so an intent queued there would stay invisible to the branch's
//! Temporal API until some later temporal mutation of the same entity forced
//! it through. A Snapshot branch therefore always records its history in the
//! write's own transaction.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::port::temporal_intent_publisher::TemporalIntentPublisher;
use std::sync::Arc;

/// How one tenant's PostgreSQL anchor commits temporal append consequences.
pub enum TemporalHistoryCommitMode {
    /// Write history into PostgreSQL inside the entity transaction.
    Native {
        /// Transactional PostgreSQL temporal recorder.
        recorder: Arc<TemporalRecorder>,
    },
    /// Commit the live branch's history as typed intents delivered through the
    /// selected sink after commit, and a Snapshot branch's inside the entity
    /// transaction.
    Deferred {
        /// Edge notification that reduces delivery latency without carrying work.
        publisher: Arc<dyn TemporalIntentPublisher>,
        /// Transactional recorder a Snapshot branch's history is written with.
        recorder: Arc<TemporalRecorder>,
    },
}

/// Where one commit's temporal history is written.
pub(crate) enum HistoryPlacement<'a> {
    /// Into the history tables, inside the commit's own transaction.
    Inline(&'a TemporalRecorder),
    /// Into the live branch's intent queue, for the deferred drain to apply,
    /// waking the drain through `publisher` once the commit lands. Only a
    /// live-branch commit is placed here.
    Queued(&'a dyn TemporalIntentPublisher),
}

impl TemporalHistoryCommitMode {
    /// Where a commit against `target` writes its temporal history.
    pub(crate) fn placement(&self, target: &ContextTarget) -> HistoryPlacement<'_> {
        match (self, target) {
            (TemporalHistoryCommitMode::Native { recorder }, ContextTarget::Live | ContextTarget::Snapshot(_))
            | (TemporalHistoryCommitMode::Deferred { recorder, .. }, ContextTarget::Snapshot(_)) => HistoryPlacement::Inline(recorder),
            (TemporalHistoryCommitMode::Deferred { publisher, .. }, ContextTarget::Live) => HistoryPlacement::Queued(publisher.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::history_mode::{HistoryPlacement, TemporalHistoryCommitMode};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_cache_config::NameCacheConfig};
    use andromeda_postgresql_temporal_store::{
        bulk_write_mode::BulkWriteMode,
        columnstore_capability::ColumnstoreSupport,
        recorder::TemporalRecorder,
        repo::TemporalRepository,
    };
    use andromeda_temporal::port::temporal_intent_publisher::NoTemporalIntentPublisher;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    /// A recorder over a pool that never connects: the placement decision only
    /// names it.
    fn recorder() -> Arc<TemporalRecorder> {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused@127.0.0.1:1/unused")
            .expect("a lazy pool is built without connecting");
        Arc::new(TemporalRecorder::new(Arc::new(TemporalRepository::new(
            &pool,
            BulkWriteMode::default(),
            Arc::new(NameCache::<AttributeName>::new(&pool, &NameCacheConfig::default())),
            Arc::new(NameCache::<EntityTypeName>::new(&pool, &NameCacheConfig::default())),
            ColumnstoreSupport::Absent,
        ))))
    }

    fn snapshot() -> ContextTarget {
        ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(4).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:history".parse().expect("a valid snapshot URI"),
        ))
    }

    fn deferred() -> TemporalHistoryCommitMode {
        TemporalHistoryCommitMode::Deferred {
            publisher: Arc::new(NoTemporalIntentPublisher),
            recorder: recorder(),
        }
    }

    #[tokio::test]
    async fn deferred_mode_queues_the_live_branchs_history() {
        assert!(matches!(deferred().placement(&ContextTarget::Live), HistoryPlacement::Queued(_)));
    }

    /// Clause 5.5.15 permits the Temporal API on a Snapshot, so its history
    /// must be readable there as soon as the write answers; nothing drains a
    /// Snapshot branch's intent queue behind it.
    #[tokio::test]
    async fn deferred_mode_records_a_snapshot_branchs_history_in_the_write_transaction() {
        assert!(matches!(deferred().placement(&snapshot()), HistoryPlacement::Inline(_)));
    }

    #[tokio::test]
    async fn native_mode_records_every_branchs_history_in_the_write_transaction() {
        let native = TemporalHistoryCommitMode::Native { recorder: recorder() };
        assert!(matches!(native.placement(&ContextTarget::Live), HistoryPlacement::Inline(_)));
        assert!(matches!(native.placement(&snapshot()), HistoryPlacement::Inline(_)));
    }
}
