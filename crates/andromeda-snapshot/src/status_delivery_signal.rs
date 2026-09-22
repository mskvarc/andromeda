//! Port for waking durable Snapshot status-notification delivery.

/// Signals the durable status-delivery drain that a Snapshot lifecycle change
/// enqueued notifications, so they are delivered on the next prompt pass rather
/// than only on the drain's idle fallback interval.
///
/// The scheduled preparation sweep wakes delivery whenever it completes a
/// preparing Snapshot, but a create-time preparation and a metadata update both
/// enqueue status rows outside that sweep. Without a signal their notifications
/// would wait for the delivery drain's fallback, which backs off toward its
/// ceiling while idle, so the service raises this signal after each such change.
/// A spurious signal is harmless: the drain claims no rows and returns to its
/// fallback.
pub trait StatusDeliverySignal: Send + Sync {
    /// Request one prompt status-delivery pass.
    fn signal(&self);
}
