//! The persisted family of one stored subscription.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Which family a stored subscription belongs to.
    ///
    /// Entity subscriptions (clause 5.8) and context-source subscriptions (clause
    /// 5.11) share one relation and are told apart by this code. The family is an
    /// Andromeda storage concept, not a specification token, so the labels are
    /// kebab-case.
    SubscriptionKindCode {
        frozen_prefix: 2,
        frozen_digest: 0xb020_f362_0255_f915,
        codes: {
            /// An entity subscription (clause 5.8).
            Entity = 1, "entity";
            /// A context-source subscription (clause 5.11).
            Csource = 2, "csource";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_kind::SubscriptionKindCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(SubscriptionKindCode::ALL.len(), 2);
        assert_eq!(SubscriptionKindCode::Entity.code(), 1);
        assert_eq!(SubscriptionKindCode::Csource.code(), 2);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(SubscriptionKindCode::try_from(0).is_err());
        assert!(SubscriptionKindCode::try_from(3).is_err());
    }
}
