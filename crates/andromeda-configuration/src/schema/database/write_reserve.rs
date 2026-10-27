//! The part of a tenant pool read requests leave to writes.
//!
//! Every request takes its connection from its tenant's one pool. Without a
//! reserve a read load takes every connection in arrival order and the writes'
//! throughput follows how many readers there are; with one, read requests hold
//! at most the rest of the pool and writes always find the reserve.

use derive_more::Into;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A percentage of a tenant pool read requests never hold.
///
/// Below 100, so a read request always has a connection to wait for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Into, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "A percentage of a tenant pool that read requests never hold, from 0 to 99.")]
#[serde(try_from = "u8", into = "u8")]
pub struct WriteReserve(u8);

/// A write reserve that would leave read requests no connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
#[error("a write reserve of {0} percent leaves read requests no connection; it must be below 100")]
pub struct WriteReserveError(u8);

impl TryFrom<u8> for WriteReserve {
    type Error = WriteReserveError;

    fn try_from(percent: u8) -> Result<WriteReserve, WriteReserveError> {
        if percent < 100 {
            Ok(WriteReserve(percent))
        } else {
            Err(WriteReserveError(percent))
        }
    }
}

impl WriteReserve {
    /// A quarter of the pool.
    pub const DEFAULT: WriteReserve = WriteReserve(25);

    /// How many connections of a pool of `max_connections` read requests may
    /// hold at once.
    ///
    /// The reserve is rounded down, so a pool too small to spare a whole
    /// connection keeps every one for reads, and a pool of at least one
    /// connection always leaves reads one.
    #[must_use]
    pub fn read_connections(self, max_connections: u32) -> u32 {
        let percent = u32::from(self.0);
        let reserved = max_connections / 100 * percent + max_connections % 100 * percent / 100;
        max_connections - reserved
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::database::write_reserve::{WriteReserve, WriteReserveError};
    use pretty_assertions::assert_eq;

    fn reserve(percent: u8) -> WriteReserve {
        WriteReserve::try_from(percent).expect("a reserve below 100 percent")
    }

    #[test]
    fn the_default_reserve_leaves_reads_three_quarters_of_the_pool() {
        assert_eq!(WriteReserve::DEFAULT.read_connections(32), 24);
        assert_eq!(WriteReserve::DEFAULT.read_connections(16), 12);
    }

    #[test]
    fn a_reserve_rounds_down_so_a_small_pool_keeps_every_connection_for_reads() {
        assert_eq!(WriteReserve::DEFAULT.read_connections(3), 3);
        assert_eq!(WriteReserve::DEFAULT.read_connections(2), 2);
        assert_eq!(WriteReserve::DEFAULT.read_connections(1), 1);
        assert_eq!(WriteReserve::DEFAULT.read_connections(4), 3);
    }

    #[test]
    fn the_largest_reserve_still_leaves_reads_one_connection() {
        assert_eq!(reserve(99).read_connections(1), 1);
        assert_eq!(reserve(99).read_connections(100), 1);
        assert_eq!(reserve(99).read_connections(u32::MAX), 42_949_673);
    }

    #[test]
    fn no_reserve_leaves_reads_the_whole_pool() {
        assert_eq!(reserve(0).read_connections(32), 32);
    }

    #[test]
    fn a_reserve_of_the_whole_pool_is_refused() {
        assert_eq!(WriteReserve::try_from(100), Err(WriteReserveError(100)));
        assert_eq!(WriteReserve::try_from(u8::MAX), Err(WriteReserveError(u8::MAX)));
    }

    #[test]
    fn a_reserve_reads_and_writes_as_its_percentage() {
        assert_eq!(serde_json::from_str::<WriteReserve>("40").ok(), Some(reserve(40)));
        assert!(serde_json::from_str::<WriteReserve>("100").is_err());
        assert_eq!(serde_json::to_string(&reserve(40)).ok().as_deref(), Some("40"));
    }
}
