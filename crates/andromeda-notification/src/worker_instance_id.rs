//! Identity assigned to one notification delivery worker.

use derive_more::{AsRef, Display};
use uuid::Uuid;

/// Stable identifier recorded while a background worker owns a database lease.
#[derive(AsRef, Clone, Debug, Display, Eq, PartialEq)]
#[as_ref(str)]
pub struct WorkerInstanceId(String);

impl WorkerInstanceId {
    /// Create the identifier used by a notification delivery worker.
    #[must_use]
    pub fn delivery(nonce: Uuid) -> WorkerInstanceId {
        WorkerInstanceId(format!("notification-delivery-worker-{nonce}"))
    }
}

#[cfg(test)]
mod tests {
    use crate::worker_instance_id::WorkerInstanceId;
    use uuid::Uuid;

    #[test]
    fn a_delivery_worker_identity_borrows_as_its_inner_string() {
        let worker = WorkerInstanceId::delivery(Uuid::nil());
        assert_eq!(
            AsRef::<str>::as_ref(&worker),
            "notification-delivery-worker-00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn a_delivery_worker_identity_displays_as_its_inner_string() {
        let worker = WorkerInstanceId::delivery(Uuid::nil());
        assert_eq!(worker.to_string(), "notification-delivery-worker-00000000-0000-0000-0000-000000000000");
    }
}
