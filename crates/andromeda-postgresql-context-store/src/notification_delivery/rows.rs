//! Validated notification delivery records and their mapper errors.

use crate::notification_delivery::claimed_event::{ClaimedEventError, ClaimedEvents};
use andromeda_context_commit::{subscription_key::SubscriptionKey, write_ordinal::WriteOrdinal};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use andromeda_notification::{
    delivery_entry::{DeliveryEntry, DeliveryKey},
    notification_event::NotificationEventKey,
};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;

/// A claimed delivery whose row could not be read back as its domain record.
///
/// The claim leased the row before reading it, so the branch and key its
/// acknowledgement addresses travel with the reason it could not be read.
#[derive(Debug, Error)]
#[error("notification delivery {delivery_key} {reason}")]
pub struct UnreadableClaimedDelivery {
    /// Context branch whose queue holds the row.
    pub target: ContextTarget,
    /// Key of the leased row.
    pub delivery_key: DeliveryKey,
    /// Why the row could not be read back.
    pub reason: ClaimedDeliveryError,
}

/// Why a claimed delivery could not be read back as its domain record.
#[derive(Debug, Error)]
pub enum ClaimedDeliveryError {
    /// The subscription identifier is not a URI permitted by clause 4.6.2.
    #[error("references an invalid subscription URI")]
    InvalidSubscriptionId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
    /// The event the delivery answers was read back but does not decode; every
    /// delivery of that event is unreadable for the same reason.
    #[error("answers event {event_key}, which {reason}")]
    UnreadableEvent {
        /// Key of the event.
        event_key: NotificationEventKey,
        /// Why the event does not decode, shared by every delivery of it.
        reason: Arc<ClaimedEventError>,
    },
    /// The claim read back no event under the key the delivery references.
    ///
    /// An invariant breach: a delivery cascades away with its event, the
    /// reap removes only events no delivery references, and the claim holds
    /// the delivery's row lock from the claim through the event read.
    #[error("answers event {event_key}, which the claim did not read back")]
    EventNotRead {
        /// Key of the event.
        event_key: NotificationEventKey,
    },
}

/// SQL decode shape of one claimed delivery and the subscription it answers,
/// as the claim returns it.
///
/// Crate-private so unvalidated database text cannot cross the persistence
/// boundary.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawClaimedDeliveryRow {
    /// Database-generated delivery key.
    pub(crate) delivery_key: i64,
    /// Raw subscription identifier decoded by the row mapper, returned as the
    /// subscription's `id`.
    #[sqlx(rename = "id")]
    pub(crate) subscription_id: String,
    /// Delivery attempts already made.
    pub(crate) attempt_count: i32,
    /// Earliest instant a further attempt may be made.
    pub(crate) next_attempt_at: DateTime<Utc>,
    /// Key of the event the delivery carries.
    pub(crate) event_key: i64,
}

impl RawClaimedDeliveryRow {
    /// Validate one claimed delivery into the entry the worker delivers,
    /// sharing the decoded event with every other delivery of it, and attach
    /// the branch `target` it was drained from.
    ///
    /// The branch is the caller's, not decoded from the row: a delivery lives in
    /// its branch's own queue, so the drain that reads `snap_<key>.notification_deliveries`
    /// already knows which Snapshot it is answering for.
    ///
    /// # Errors
    ///
    /// Returns [`UnreadableClaimedDelivery`] when the subscription identifier
    /// is not a valid URI, or the event the delivery answers does not decode
    /// or was not read back, carrying the branch and key the leased row's
    /// acknowledgement addresses. It is boxed because it is rare and far wider
    /// than the success path.
    pub(crate) fn into_entry(self, events: &ClaimedEvents, target: &ContextTarget) -> Result<DeliveryEntry, Box<UnreadableClaimedDelivery>> {
        let delivery_key = DeliveryKey::from(self.delivery_key);
        self.decode(events, target).map_err(|reason| {
            Box::new(UnreadableClaimedDelivery {
                target: target.clone(), // clone: the error outlives the borrowed target, so it owns a copy.
                delivery_key,
                reason,
            })
        })
    }

    /// Validate every stored value of one claimed delivery and find its event.
    fn decode(self, events: &ClaimedEvents, target: &ContextTarget) -> Result<DeliveryEntry, ClaimedDeliveryError> {
        let subscription_id: Identifier = self
            .subscription_id
            .parse()
            .map_err(|source| ClaimedDeliveryError::InvalidSubscriptionId { source })?;
        let event_key = NotificationEventKey::from(self.event_key);
        let event = match events.get(event_key) {
            Some(Ok(event)) => Arc::clone(event),
            Some(Err(reason)) => {
                return Err(ClaimedDeliveryError::UnreadableEvent {
                    event_key,
                    reason: Arc::clone(reason),
                });
            }
            None => return Err(ClaimedDeliveryError::EventNotRead { event_key }),
        };
        Ok(DeliveryEntry::builder()
            .target(target.clone()) // clone: the entry outlives the borrowed target, so it owns a copy.
            .key(DeliveryKey::from(self.delivery_key))
            .subscription_id(subscription_id)
            .attempt_count(self.attempt_count)
            .next_attempt_at(self.next_attempt_at)
            .event(event)
            .build())
    }
}

/// One notification event to write, and the subscriptions it is due to.
///
/// The subscriptions are named by the surrogate key `notification_deliveries`
/// references, resolved by the matcher from the loaded subscription: the insert
/// then writes every delivery of the write from two parallel arrays with no
/// lookup of its own.
#[derive(Debug)]
pub struct NewNotificationEvent<'a> {
    /// Position of this event within its write, unique across the whole batch.
    pub ordinal: WriteOrdinal,
    /// What the write did to the Entity (clause 5.8.6).
    pub change: &'a EntityChange,
    /// Entity whose change the event records.
    pub entity_id: &'a Identifier,
    /// Dictionary key of each changed instance's attribute, position for
    /// position with `change.instances`.
    pub change_attribute_name_keys: Vec<AttributeNameKey>,
    /// Entity state after the change, or the state a deletion removed.
    pub entity_snapshot: Option<&'a EntityState>,
    /// Entity state before the change, for `showChanges`.
    pub previous_snapshot: Option<&'a EntityState>,
    /// The clause 4.8 system attributes of the entity as the change left it,
    /// or as a deletion removed it.
    pub sys_attrs_snapshot: Option<&'a SysAttrsSnapshot>,
    /// Subscriptions this event is due to, in matcher order.
    pub subscription_keys: Vec<SubscriptionKey>,
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        claimed_event::{
            ClaimedEventError,
            ClaimedEvents,
            RawClaimedEventRow,
            ReadEvents,
            tests::{event_row, records, temperature},
        },
        rows::{ClaimedDeliveryError, RawClaimedDeliveryRow, UnreadableClaimedDelivery},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_notification::{delivery_entry::DeliveryKey, notification_event::NotificationEventKey};
    use andromeda_postgresql_attribute_instance::name_keys::KeyNames;
    use chrono::Utc;
    use std::sync::Arc;

    fn delivery(delivery_key: i64, event_key: i64) -> RawClaimedDeliveryRow {
        RawClaimedDeliveryRow {
            delivery_key,
            subscription_id: "urn:ngsi-ld:Subscription:1".to_owned(),
            attempt_count: 2,
            next_attempt_at: Utc::now(),
            event_key,
        }
    }

    fn claimed(rows: impl IntoIterator<Item = RawClaimedEventRow>, names: &KeyNames) -> ClaimedEvents {
        ReadEvents::read(rows).assemble(names)
    }

    #[test]
    fn a_valid_row_decodes_every_domain_value() {
        let events = claimed([event_row()], &KeyNames::default());
        let entry = delivery(7, 3).into_entry(&events, &ContextTarget::Live).expect("a valid claimed delivery");
        pretty_assertions::assert_eq!(entry.key, DeliveryKey::from(7));
        pretty_assertions::assert_eq!(entry.subscription_id.as_str(), "urn:ngsi-ld:Subscription:1");
        pretty_assertions::assert_eq!(entry.attempt_count, 2);
        pretty_assertions::assert_eq!(entry.event.key, NotificationEventKey::from(3_i64));
        pretty_assertions::assert_eq!(entry.target, ContextTarget::Live);
    }

    #[test]
    fn deliveries_of_one_event_share_one_decoded_state() {
        let state = temperature(20);
        let (mut records, names) = records(&[&state]);
        let mut row = event_row();
        row.event_key = 7;
        row.entity_snapshot = records.pop();
        let events = claimed([row], &names);
        let entries = [1, 2, 3].map(|key| delivery(key, 7).into_entry(&events, &ContextTarget::Live).expect("a readable delivery"));
        let states = entries
            .iter()
            .map(|entry| entry.event.entity_snapshot.as_ref().expect("the stored state"))
            .collect::<Vec<_>>();
        assert!(
            states.iter().all(|state| Arc::ptr_eq(state, states[0])),
            "every delivery shares one decoded state"
        );
        assert!(entries.iter().all(|entry| Arc::ptr_eq(&entry.event, &entries[0].event)));
        pretty_assertions::assert_eq!(entries.map(|entry| entry.key), [1, 2, 3].map(DeliveryKey::from));
    }

    #[test]
    fn an_unreadable_event_makes_each_of_its_deliveries_unreadable() {
        let state = temperature(20);
        let (mut records, _) = records(&[&state]);
        let mut row = event_row();
        row.event_key = 7;
        row.entity_snapshot = records.pop();
        let events = claimed([row], &KeyNames::default());
        let refused = [4, 5].map(|key| delivery(key, 7).into_entry(&events, &ContextTarget::Live).expect_err("an unreadable event"));
        for (unreadable, key) in refused.iter().zip([4, 5]) {
            pretty_assertions::assert_eq!(unreadable.delivery_key, DeliveryKey::from(key));
            pretty_assertions::assert_eq!(unreadable.target, ContextTarget::Live);
            assert!(
                matches!(&unreadable.reason, ClaimedDeliveryError::UnreadableEvent { event_key, reason }
                    if *event_key == NotificationEventKey::from(7_i64) && matches!(reason.as_ref(), ClaimedEventError::UnreadableSnapshot { .. })),
                "{unreadable}"
            );
        }
    }

    #[test]
    fn a_delivery_whose_event_was_not_read_is_an_invariant_breach() {
        let events = claimed([event_row()], &KeyNames::default());
        let unreadable = delivery(9, 5)
            .into_entry(&events, &ContextTarget::Live)
            .expect_err("an event the claim did not read");
        assert!(matches!(
            *unreadable,
            UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::EventNotRead { event_key },
                ..
            } if event_key == NotificationEventKey::from(5_i64)
        ));
    }

    #[test]
    fn an_invalid_subscription_identifier_is_rejected() {
        let mut raw = delivery(7, 3);
        raw.subscription_id = "not a uri".to_owned();
        let events = claimed([event_row()], &KeyNames::default());
        assert!(matches!(
            raw.into_entry(&events, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::InvalidSubscriptionId { .. },
                ..
            })
        ));
    }

    #[test]
    fn the_mapper_failure_names_the_delivery_and_the_event_it_could_not_read() {
        let mut raw = event_row();
        raw.lifecycle = 99;
        let events = claimed([raw], &KeyNames::default());
        let error = delivery(7, 3).into_entry(&events, &ContextTarget::Live).expect_err("an unreadable lifecycle");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "notification delivery 7 answers event 3, which carries an unreadable lifecycle"
        );
    }

    /// The claim leased the row before reading it, so the key and branch its
    /// acknowledgement addresses survive the failure to read it.
    #[test]
    fn an_unreadable_row_keeps_the_key_and_branch_its_acknowledgement_addresses() {
        let mut raw = delivery(7, 3);
        raw.subscription_id = "not a uri".to_owned();
        let error = raw
            .into_entry(&ClaimedEvents::default(), &ContextTarget::Live)
            .expect_err("an unreadable subscription identifier");
        pretty_assertions::assert_eq!(error.delivery_key, DeliveryKey::from(7));
        pretty_assertions::assert_eq!(error.target, ContextTarget::Live);
    }
}
