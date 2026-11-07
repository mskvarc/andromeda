//! The number a subscription's `notificationTrigger` list stores each trigger
//! condition as.
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
pub(crate) const fn stored_trigger(trigger: NotificationTrigger) -> NotificationTriggerCode {
    match trigger {
        NotificationTrigger::EntityCreated => NotificationTriggerCode::EntityCreated,
        NotificationTrigger::EntityUpdated => NotificationTriggerCode::EntityUpdated,
        NotificationTrigger::EntityDeleted => NotificationTriggerCode::EntityDeleted,
        NotificationTrigger::AttributeCreated => NotificationTriggerCode::AttributeCreated,
        NotificationTrigger::AttributeUpdated => NotificationTriggerCode::AttributeUpdated,
        NotificationTrigger::AttributeDeleted => NotificationTriggerCode::AttributeDeleted,
    }
}

/// The stored `SMALLINT[]` of a subscription's trigger set, or `None` when the
/// subscription carries no explicit set (matching every trigger).
#[must_use]
pub(crate) fn stored_trigger_list(triggers: Option<&[NotificationTrigger]>) -> Option<Vec<i16>> {
    triggers.map(|triggers| triggers.iter().map(|trigger| stored_trigger(*trigger).code()).collect())
}

/// The trigger condition a stored registry member names.
#[must_use]
pub(crate) const fn trigger_of(code: NotificationTriggerCode) -> NotificationTrigger {
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
    use crate::subscription_store::trigger_code::{stored_trigger, stored_trigger_list, trigger_of};
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
    fn a_trigger_set_is_stored_as_its_registry_numbers_in_the_order_written() {
        let triggers = [NotificationTrigger::AttributeUpdated, NotificationTrigger::EntityCreated];
        pretty_assertions::assert_eq!(
            stored_trigger_list(Some(&triggers)),
            Some(vec![
                stored_trigger(NotificationTrigger::AttributeUpdated).code(),
                stored_trigger(NotificationTrigger::EntityCreated).code()
            ])
        );
    }

    #[test]
    fn no_explicit_trigger_set_stores_nothing() {
        pretty_assertions::assert_eq!(stored_trigger_list(None), None);
    }

    #[test]
    fn the_registry_and_the_specification_vocabulary_have_the_same_size() {
        pretty_assertions::assert_eq!(NotificationTriggerCode::ALL.len(), NotificationTrigger::iter().count());
    }
}
