//! What finishes in the in-flight set of a delivery pass.
//!
//! A pass has two kinds of work in flight: the attempts at the deliveries it
//! claimed, each holding a slot, and at most one round to the queue. Each
//! finishes as one event the pass takes in before it plans the next round.

use crate::{
    delivery_claim_order::DeliveryClaimOrder,
    delivery_round::SettledRound,
    delivery_settlement::DeliverySettlement,
    delivery_slot_share::SlotOccupant,
    error::NotificationError,
};
use std::num::NonZeroUsize;

/// One thing that finished in a pass's in-flight set.
pub(crate) enum SlotEvent {
    /// A claimed delivery was attempted, and waits to be settled.
    Attempted(Box<AttemptEnded>),
    /// A round to the queue came back.
    RoundEnded(Box<RoundEnded>),
}

/// How the attempt at one claimed delivery ended, and the slot it holds
/// until its outcome is settled.
pub(crate) struct AttemptEnded {
    /// How the attempt ended, or why it could not be settled.
    pub(crate) settlement: Result<DeliverySettlement, NotificationError>,
    /// The Subscription whose slot the delivery holds, `None` for a delivery
    /// whose row could not be read back.
    pub(crate) occupant: Option<SlotOccupant>,
}

/// A settled delivery waiting for the next round, and the slot it holds
/// until that round is issued.
pub(crate) struct AwaitingAcknowledgement {
    /// What the queue records of it.
    pub(crate) settlement: DeliverySettlement,
    /// The Subscription whose slot it holds, `None` for a delivery whose row
    /// could not be read back.
    pub(crate) occupant: Option<SlotOccupant>,
}

/// The claim one round asked for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClaimAsked {
    /// Deliveries the claim asked for.
    pub(crate) width: NonZeroUsize,
    /// The order the claim took them in.
    pub(crate) order: DeliveryClaimOrder,
}

/// A round that came back, and the claim it asked for.
pub(crate) struct RoundEnded {
    /// The claim the round asked for, `None` for one that only
    /// acknowledged.
    pub(crate) claim: Option<ClaimAsked>,
    /// What the queue answered, or why the settlements could not be
    /// recorded.
    pub(crate) answer: Result<SettledRound, NotificationError>,
}
