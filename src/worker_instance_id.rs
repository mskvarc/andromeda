//! Identity assigned to one background-worker process.

use andromeda_ngsi_ld_lexicon::uri::Uri;
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

    /// Create the identifier used by a tenant's pending-side-effects worker.
    pub fn pending_side_effects(source_id: &Uri, nonce: Uuid) -> WorkerInstanceId {
        WorkerInstanceId(format!("{source_id}-{nonce}"))
    }
}
