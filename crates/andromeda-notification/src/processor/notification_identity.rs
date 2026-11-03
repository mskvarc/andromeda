//! Which identifier a notification carries.
//!
//! A live notification is sent once and takes a random identifier. A durably
//! queued one may be attempted again after a timed-out answer or a broker stop,
//! so every attempt derives the identifier from the queue's own values and an
//! endpoint can recognise the repeat.

use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_delivery::{
    notification_body::NotificationWriteError,
    request::{DeliveryRequest, NotificationDelivery},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId};
use andromeda_ngsi_ld_model::notification::NotificationData;
use chrono::{DateTime, Utc};

/// Where a notification comes from, which decides its identifier.
#[derive(Clone, Copy, Debug)]
pub(crate) enum NotificationIdentity {
    /// A notification sent as its change commits, once.
    Live,
    /// The notification a queued delivery owes, attempted until it is accepted.
    Queued {
        /// The write that made the change.
        write_id: WriteId,
        /// The change's position within that write.
        event_ordinal: WriteOrdinal,
    },
}

impl NotificationIdentity {
    /// The notification carrying `data` to `subscription`, written by `delivery`
    /// under the identifier this identity gives it.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationWriteError`] when the identifier cannot be minted
    /// or derived, or the notification cannot be written.
    pub(crate) fn notification(
        self,
        delivery: &NotificationDelivery<'_>,
        subscription: &Identifier,
        data: NotificationData,
        notified_at: DateTime<Utc>,
    ) -> Result<DeliveryRequest, NotificationWriteError> {
        match self {
            NotificationIdentity::Live => delivery.notification(data, notified_at),
            NotificationIdentity::Queued { write_id, event_ordinal } => {
                let id = NotificationId::queued(write_id.as_uuid(), event_ordinal.get(), subscription)?;
                delivery.notification_with_id(id, data, notified_at)
            }
        }
    }
}
