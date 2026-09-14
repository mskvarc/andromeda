//! The persisted trigger condition of one notification record.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The change that made a notification due (clause 5.3.3).
    ///
    /// The same vocabulary serves both sides of clause 5.3.3 — the conditions a
    /// subscription listens for and the one condition an emitted change carries
    /// — so a subscription's stored trigger set and a queued notification's
    /// trigger are numbered from one registry rather than two.
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
