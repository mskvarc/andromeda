//! Identity assigned to one notification outbox drain.

use derive_more::{AsRef, Display};
use uuid::Uuid;

/// Stable identifier recorded while a background worker owns a database lease.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display)]
#[as_ref(str)]
pub struct WorkerInstanceId(String);

impl WorkerInstanceId {
    /// Create the identifier used by a notification-outbox delivery worker.
    pub fn outbox(nonce: Uuid) -> WorkerInstanceId {
        WorkerInstanceId(format!("outbox-worker-{nonce}"))
    }
}
