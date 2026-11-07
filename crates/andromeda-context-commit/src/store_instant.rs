//! An instant read from the store's own clock.
//!
//! Brokers sharing a tenant database do not share a clock. Two writes are
//! ordered across brokers only by an instant every one of them takes from the
//! same source, so the instants that order an entity change against a
//! Subscription's lifecycle are both the transaction timestamp of the store
//! transaction that committed them, never a broker's wall clock.

use chrono::{DateTime, Utc};

/// The store's transaction timestamp of the transaction that committed a write.
///
/// Every write of one transaction shares it, and a transaction that began
/// after another committed reads a later one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StoreInstant(DateTime<Utc>);

impl StoreInstant {
    /// The instant the store answered.
    #[must_use]
    pub const fn new(instant: DateTime<Utc>) -> StoreInstant {
        StoreInstant(instant)
    }

    /// The instant.
    #[must_use]
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::store_instant::StoreInstant;
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_store_instant_orders_as_the_instant_it_holds() {
        let earlier = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let later = Utc.timestamp_opt(1_700_000_001, 0).single().expect("valid instant");
        assert!(StoreInstant::new(earlier) < StoreInstant::new(later));
        pretty_assertions::assert_eq!(StoreInstant::new(earlier).get(), earlier);
    }
}
