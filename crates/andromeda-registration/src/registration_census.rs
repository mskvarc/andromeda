//! How many registrations a tenant holds, kept so that a distributed operation
//! in a tenant holding none reads no registration at all.
//!
//! The count is never below the number of registrations the store holds. A
//! creation is counted before its write reaches the store and given back only
//! when the store refuses it as a duplicate, which writes nothing. A deletion,
//! on request or by the expiry sweep, is counted once the store reports it
//! applied. A write whose outcome is unknown stays counted. The count may
//! therefore exceed what the store holds, which costs a lookup, but never falls
//! short of it, which would keep a request from a registration it has to reach
//! (clause 4.3.6). A registration past its `expiresAt` stays counted until the
//! sweep deletes it, as the count a tenant opens with includes it.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// How many registrations a tenant holds, never fewer than its store holds.
///
/// Cloning shares the count, so the registration service, the expiry sweep and
/// the forwarding of one tenant keep one count.
#[derive(Clone, Debug)]
pub struct RegistrationCensus {
    /// The registrations counted.
    held: Arc<AtomicU64>,
}

impl RegistrationCensus {
    /// A census of `held` registrations, the number the store holds when the
    /// tenant opens, those past their `expiresAt` included.
    #[must_use]
    pub fn new(held: u64) -> RegistrationCensus {
        RegistrationCensus {
            held: Arc::new(AtomicU64::new(held)),
        }
    }

    /// Whether the tenant holds no registration, so no distributed operation
    /// has one to reach.
    #[must_use]
    pub fn holds_none(&self) -> bool {
        self.held.load(Ordering::SeqCst) == 0
    }

    /// Count a registration whose creation is about to reach the store.
    pub(crate) fn admit(&self) {
        self.held.fetch_add(1, Ordering::SeqCst);
    }

    /// Stop counting `count` registrations, returning how many are still
    /// counted. The count never goes below none.
    pub(crate) fn release(&self, count: usize) -> u64 {
        let released = u64::try_from(count).unwrap_or(u64::MAX);
        let previous = self.held.update(Ordering::SeqCst, Ordering::SeqCst, |held| held.saturating_sub(released));
        previous.saturating_sub(released)
    }
}

#[cfg(test)]
mod tests {
    use crate::registration_census::RegistrationCensus;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tenant_opening_without_registrations_holds_none() {
        assert!(RegistrationCensus::new(0).holds_none());
        assert!(!RegistrationCensus::new(3).holds_none());
    }

    #[test]
    fn an_admitted_creation_is_counted_until_released() {
        let census = RegistrationCensus::new(0);
        census.admit();
        assert!(!census.holds_none());
        assert_eq!(census.release(1), 0);
        assert!(census.holds_none());
    }

    #[test]
    fn a_release_never_takes_the_count_below_none() {
        let census = RegistrationCensus::new(2);
        assert_eq!(census.release(5), 0);
        assert!(census.holds_none());
    }

    #[test]
    fn a_clone_shares_the_count() {
        let census = RegistrationCensus::new(0);
        let shared = census.clone();
        shared.admit();
        assert!(!census.holds_none());
        assert_eq!(census.release(0), 1);
    }
}
