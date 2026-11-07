//! How long a tenant runtime's teardown may take.

use tokio::time::Instant;

/// The bound a tenant runtime's teardown stops its background tasks within.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeardownBound {
    /// Each task stops within its own grace period: the runtime alone is
    /// unloaded, and the broker keeps running.
    GracePeriods,
    /// Each task stops within its own grace period and by this instant: the
    /// broker is stopping, and every loaded runtime shares its deadline.
    Deadline(Instant),
}
