//! The narrow key a stored Subscription is referenced by.
//!
//! It lives with the commit vocabulary because a committing write carries it:
//! every subscription a write matched is named by this key on the way into the
//! notification queue, and the store resolves nothing on the way.

use derive_more::Display;
use thiserror::Error;

/// The surrogate key of one stored Subscription.
///
/// A Subscription's external identity is its URI (clause 5.2.12), and that is
/// what every request, notification payload and lifecycle change carries. This
/// key is the identity the notification queue carries instead: one delivery row
/// exists per matched Subscription per affected Entity, so the queue repeats
/// this reference more often than any other relation in the schema, and an
/// eight-byte generated number costs a fraction of the URI text in tuples,
/// indexes and joins.
///
/// The key is assigned once by the store and never reassigned — a reused number
/// would hand one Subscription's queued deliveries to another. It is never
/// rendered over the API and never accepted from a client; it enters the broker
/// only on the read that loads a Subscription and leaves it only on the write
/// that enqueues a delivery, which is why a [`SubscriptionMatch`]
/// (`crate::subscription_match`) carries the key rather than the URI.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct SubscriptionKey(i64);

impl SubscriptionKey {
    /// Return the stored representation.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SubscriptionKey {
    type Error = SubscriptionKeyOutOfRange;

    fn try_from(stored: i64) -> Result<SubscriptionKey, SubscriptionKeyOutOfRange> {
        if stored > 0 {
            Ok(SubscriptionKey(stored))
        } else {
            Err(SubscriptionKeyOutOfRange)
        }
    }
}

/// A subscription key was outside the range the store assigns.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("a subscription key is a positive number")]
pub struct SubscriptionKeyOutOfRange;

#[cfg(test)]
mod tests {
    use crate::subscription_key::{SubscriptionKey, SubscriptionKeyOutOfRange};

    #[test]
    fn a_stored_key_round_trips() {
        assert_eq!(SubscriptionKey::try_from(4_294_967_296_i64).map(SubscriptionKey::get), Ok(4_294_967_296));
    }

    #[test]
    fn zero_and_negative_keys_are_refused() {
        assert_eq!(SubscriptionKey::try_from(0), Err(SubscriptionKeyOutOfRange));
        assert_eq!(SubscriptionKey::try_from(-1), Err(SubscriptionKeyOutOfRange));
    }

    #[test]
    fn a_key_renders_as_the_number_it_stores() {
        assert_eq!(SubscriptionKey::try_from(17).map(|key| key.to_string()), Ok("17".to_owned()));
    }
}
