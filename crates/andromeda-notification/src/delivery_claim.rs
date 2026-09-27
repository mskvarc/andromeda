//! What a claim hands the delivery drain for each delivery it leased.
//!
//! A claim leases a delivery before it reads the delivery back, so a delivery
//! whose stored row cannot be read back as a domain value is leased to the
//! claiming worker all the same. It is handed back rather than skipped so the
//! drain can acknowledge it: a skipped delivery keeps its lease until the term
//! runs out and is then claimed again, with its attempt count unchanged, at
//! every lease expiry for as long as the row exists -- and until then the
//! tenant's pending work keeps reporting it due.

use crate::delivery_entry::{DeliveryEntry, DeliveryKey};
use andromeda_context_space::context_target::ContextTarget;

/// One delivery a claim leased to the claiming worker.
#[derive(Clone, Debug)]
pub enum DeliveryClaim {
    /// The delivery and the event it carries were read back.
    Readable(Box<DeliveryEntry>),
    /// The delivery's stored row could not be read back as a domain value.
    Unreadable(UnreadableDelivery),
}

/// A leased delivery whose stored row could not be read back.
///
/// Nothing a later attempt does can repair a stored value, so the drain
/// acknowledges it as exhausted with [`UnreadableDelivery::diagnostic`] as its
/// recorded failure.
#[derive(Clone, Debug)]
pub struct UnreadableDelivery {
    /// Context branch whose queue holds the row.
    pub target: ContextTarget,
    /// Key of the leased row, the subject of its acknowledgement.
    pub key: DeliveryKey,
    /// Why the row could not be read back, for the log and the stored failure.
    pub diagnostic: String,
}
