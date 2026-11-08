//! The persisted trigger conditions a subscription listens for.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// A condition a subscription listens for (clause 5.2.12).
    ///
    /// A queued notification does not store a trigger: one change raises any
    /// number of them, and what it stores is the change itself (see
    /// `EntityLifecycleCode` and `AttributeChangeKindCode`).
    ///
    /// The labels are the specification's camel-case tokens, since these are
    /// the values a subscription document carries on the wire.
    NotificationTriggerCode {
        frozen_prefix: 6,
        frozen_digest: 0xfefb_9c99_f6cf_15f9,
        codes: {
            /// An entity matching the subscription was created.
            EntityCreated = 1, "entityCreated";
            /// An entity matching the subscription was updated.
            EntityUpdated = 2, "entityUpdated";
            /// An entity matching the subscription was deleted.
            EntityDeleted = 3, "entityDeleted";
            /// A watched attribute was created on a matching entity.
            AttributeCreated = 4, "attributeCreated";
            /// A watched attribute was updated on a matching entity.
            AttributeUpdated = 5, "attributeUpdated";
            /// A watched attribute was deleted from a matching entity.
            AttributeDeleted = 6, "attributeDeleted";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_trigger::NotificationTriggerCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_registry_covers_every_trigger_condition_the_specification_defines() {
        assert_eq!(NotificationTriggerCode::ALL.len(), 6);
    }

    #[test]
    fn the_trigger_conditions_are_numbered_as_the_migration_writes_them() {
        assert_eq!(NotificationTriggerCode::EntityCreated.code(), 1);
        assert_eq!(NotificationTriggerCode::AttributeDeleted.code(), 6);
    }

    #[test]
    fn the_labels_are_the_tokens_a_subscription_document_carries() {
        assert_eq!(NotificationTriggerCode::EntityCreated.label(), "entityCreated");
        assert_eq!(NotificationTriggerCode::AttributeDeleted.label(), "attributeDeleted");
    }

    #[test]
    fn zero_is_not_a_trigger_condition() {
        assert!(NotificationTriggerCode::try_from(0).is_err());
    }

    #[test]
    fn a_trigger_condition_a_newer_release_might_add_fails_to_decode() {
        assert!(NotificationTriggerCode::try_from(7).is_err());
    }
}
