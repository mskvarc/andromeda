//! Row shapes returned by [`PendingSideEffectsRepository`](super::repo::PendingSideEffectsRepository).

use serde_json::Value;
use uuid::Uuid;

/// A side-effect job leased for processing. `payload` carries the
/// [`PersistedBatchSideEffectJob`](super::payload::PersistedBatchSideEffectJob)
/// shape; parse via `serde_json::from_value`.
#[derive(Debug, Clone)]
pub struct LeasedSideEffect {
    pub id: Uuid,
    pub attempts: i16,
    pub payload: Value,
}
