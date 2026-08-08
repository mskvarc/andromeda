//! Row shapes returned by the side-effect repository.

use serde_json::Value;
use uuid::Uuid;

/// A side-effect job leased for processing. `payload` carries the
/// persisted batch side-effect job shape; parse via `serde_json::from_value`.
#[derive(Debug, Clone)]
pub struct LeasedSideEffect {
    /// Unique row identifier.
    pub id: Uuid,
    /// Number of processing attempts so far.
    pub attempts: i16,
    /// JSONB payload containing the serialised side-effect job.
    pub payload: Value,
}
