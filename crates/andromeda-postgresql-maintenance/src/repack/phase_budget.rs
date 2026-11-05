//! How long a repack's long statements may run.
//!
//! The copy and the index builds read and write the whole relation, so their
//! budget grows with its size: the time the relation takes at a floor
//! throughput, kept between a floor that covers a small relation's fixed
//! costs and a ceiling past which a run is better abandoned than left
//! holding the database's freeze horizon. The budget is each statement's
//! `statement_timeout`, and the run as a whole is judged against it when a
//! Snapshot branch must outlive the run.

use std::time::Duration;

/// The slowest throughput a copy or an index build is expected to reach, in
/// bytes per second: a disk shared with the broker's own writes.
const FLOOR_BYTES_PER_SECOND: u64 = 8 * 1_048_576;

/// The shortest budget, whatever the relation's size.
const MINIMUM: Duration = Duration::from_mins(30);

/// The longest budget, whatever the relation's size.
const MAXIMUM: Duration = Duration::from_hours(6);

/// The `statement_timeout` of a repack's copy and of each of its index
/// builds, for a relation of `relation_bytes`.
#[must_use]
pub(crate) fn phase_budget(relation_bytes: u64) -> Duration {
    Duration::from_secs(relation_bytes / FLOOR_BYTES_PER_SECOND).clamp(MINIMUM, MAXIMUM)
}

#[cfg(test)]
mod tests {
    use crate::repack::phase_budget::phase_budget;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    const GIB: u64 = 1_073_741_824;

    /// 1 GiB at 8 MiB/s is 128 s, under the floor.
    #[test]
    fn a_small_relation_gets_the_floor() {
        assert_eq!(phase_budget(GIB), Duration::from_mins(30));
        assert_eq!(phase_budget(0), Duration::from_mins(30));
    }

    /// 32 GiB at 8 MiB/s is 4,096 s, about 68 minutes.
    #[test]
    fn a_large_relation_gets_its_time_at_the_floor_throughput() {
        assert_eq!(phase_budget(32 * GIB), Duration::from_secs(4_096));
    }

    /// 500 GiB at 8 MiB/s is 64,000 s, past the six-hour ceiling.
    #[test]
    fn a_huge_relation_is_clamped_to_six_hours() {
        assert_eq!(phase_budget(500 * GIB), Duration::from_hours(6));
    }
}
