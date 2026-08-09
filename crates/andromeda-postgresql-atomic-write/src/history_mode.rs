//! Selection of native or deferred temporal history within the PostgreSQL anchor.

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
    /// Commit typed intents and deliver them through the selected sink after commit.
    Deferred {
        /// Edge notification that reduces delivery latency without carrying work.
        publisher: Arc<dyn TemporalIntentPublisher>,
    },
}
