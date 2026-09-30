//! The persisted lifecycle state of one stored subscription.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The lifecycle state of a subscription (clause 5.2.12 `status`).
    ///
    /// The labels are the specification's own status tokens.
    SubscriptionStatusCode {
        frozen_prefix: 3,
        frozen_digest: 0x8cdf_9287_4e6c_635c,
        codes: {
            /// The subscription is active and notifies.
            Active = 1, "active";
            /// The subscriber paused the subscription.
            Paused = 2, "paused";
            /// The subscription's `expiresAt` has passed.
            Expired = 3, "expired";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_status::SubscriptionStatusCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(SubscriptionStatusCode::ALL.len(), 3);
        assert_eq!(SubscriptionStatusCode::Active.code(), 1);
        assert_eq!(SubscriptionStatusCode::Expired.code(), 3);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(SubscriptionStatusCode::try_from(0).is_err());
        assert!(SubscriptionStatusCode::try_from(4).is_err());
    }
}
