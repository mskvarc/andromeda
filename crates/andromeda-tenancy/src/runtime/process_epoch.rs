//! The instant the millisecond stamps of the runtime cache are counted from.

use std::{sync::LazyLock, time::Instant};

/// The instant every millisecond stamp the runtime cache holds in an atomic is
/// measured from.
///
/// `Instant` cannot live in an atomic, and a runtime's last use is written on
/// every request. Storing an offset in milliseconds from a process-wide epoch
/// makes the write a relaxed atomic store rather than a lock.
pub(super) static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Milliseconds elapsed since the process epoch.
pub(super) fn now_millis() -> u64 {
    // `Duration::as_millis` answers in `u128`. The whole-second and sub-second
    // halves are each `u64`-representable on their own, so the stamp is
    // assembled from them rather than narrowed from a wider integer.
    let elapsed = EPOCH.elapsed();
    elapsed.as_secs().saturating_mul(1_000).saturating_add(u64::from(elapsed.subsec_millis()))
}

#[cfg(test)]
mod tests {
    use crate::runtime::process_epoch::now_millis;

    #[test]
    fn the_stamp_never_runs_backwards() {
        let earlier = now_millis();
        assert!(now_millis() >= earlier);
    }
}
