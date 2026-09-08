//! The number `notification_events.trigger` stores a trigger condition as.
//!
//! The domain vocabulary of clause 5.3.3 and the migration-owned registry that
//! numbers it are variant-for-variant identical, and the translation between
//! them is still written out by hand in both directions. A blanket conversion
//! keyed on the number would let a trigger condition a later revision of the
//! specification adds pass silently into storage as some other condition;
//! written as exhaustive matches, that addition fails to compile here, which is
//! where the decision about its persisted number belongs.

use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_postgresql_codes::notification_trigger::NotificationTriggerCode;

/// The registry member the schema persists for a trigger condition.
#[must_use]
pub const fn stored_trigger(trigger: NotificationTrigger) -> NotificationTriggerCode {
    match trigger {
        NotificationTrigger::EntityCreated => NotificationTriggerCode::EntityCreated,
        NotificationTrigger::EntityUpdated => NotificationTriggerCode::EntityUpdated,
        NotificationTrigger::EntityDeleted => NotificationTriggerCode::EntityDeleted,
        NotificationTrigger::AttributeCreated => NotificationTriggerCode::AttributeCreated,
        NotificationTrigger::AttributeUpdated => NotificationTriggerCode::AttributeUpdated,
        NotificationTrigger::AttributeDeleted => NotificationTriggerCode::AttributeDeleted,
    }
}

/// The trigger condition a stored registry member names.
#[must_use]
pub const fn trigger_of(code: NotificationTriggerCode) -> NotificationTrigger {
    match code {
        NotificationTriggerCode::EntityCreated => NotificationTrigger::EntityCreated,
        NotificationTriggerCode::EntityUpdated => NotificationTrigger::EntityUpdated,
        NotificationTriggerCode::EntityDeleted => NotificationTrigger::EntityDeleted,
        NotificationTriggerCode::AttributeCreated => NotificationTrigger::AttributeCreated,
        NotificationTriggerCode::AttributeUpdated => NotificationTrigger::AttributeUpdated,
        NotificationTriggerCode::AttributeDeleted => NotificationTrigger::AttributeDeleted,
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::trigger_code::{stored_trigger, trigger_of};
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_postgresql_codes::notification_trigger::NotificationTriggerCode;
    use strum::IntoEnumIterator;

    #[test]
    fn every_trigger_condition_survives_a_round_trip_through_its_stored_number() {
        for trigger in NotificationTrigger::iter() {
            pretty_assertions::assert_eq!(trigger_of(stored_trigger(trigger)), trigger);
        }
    }

    #[test]
    fn the_stored_number_of_a_trigger_is_the_one_its_label_is_registered_under() {
        for trigger in NotificationTrigger::iter() {
            pretty_assertions::assert_eq!(stored_trigger(trigger).label(), trigger.to_string());
        }
    }

    #[test]
    fn the_registry_and_the_specification_vocabulary_have_the_same_size() {
        pretty_assertions::assert_eq!(NotificationTriggerCode::ALL.len(), NotificationTrigger::iter().count());
    }
}
