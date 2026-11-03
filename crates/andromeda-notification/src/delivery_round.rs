//! What one round trip to the delivery queue asks for and what it answers.
//!
//! A pass goes to the queue once for everything it has to say: the
//! settlements of the deliveries whose attempts ended, and the claim that
//! refills the slots they free. The queue records the settlements first and
//! claims after them, in one transaction, so a claim sees the holds those
//! settlements take or release, and one round trip serves both.

use crate::{
    delivery_claim::DeliveryClaim,
    delivery_claim_order::DeliveryClaimOrder,
    delivery_lease::LeaseTerm,
    error::NotificationError,
    held_events::HeldEvents,
};
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use std::num::NonZeroU32;

/// The claim a round asks for beside its settlements.
#[derive(Clone, Copy, Debug)]
pub struct ClaimRequest<'held> {
    /// How long each claimed delivery is leased for, from the claim.
    pub lease: LeaseTerm,
    /// The most deliveries to claim.
    pub width: NonZeroU32,
    /// Which due deliveries are taken first: [`DeliveryClaimOrder::Queue`]
    /// the ones that became claimable first,
    /// [`DeliveryClaimOrder::FewestInFlight`] those of the Subscriptions with
    /// the fewest deliveries leased to the claiming worker first, passing over
    /// those its ceiling excludes.
    pub order: DeliveryClaimOrder,
    /// The events the claiming pass already holds: a delivery of one of them
    /// is handed over with it rather than reading it again, and every event
    /// the claim reads is added to it.
    pub held: &'held HeldEvents,
}

/// What the queue answered one round.
#[derive(Debug)]
pub struct SettledRound {
    /// Whether each settlement was recorded under its lease, in the order
    /// given: [`LeaseAcknowledgement::Lost`] for a delivery whose lease lapsed
    /// or another worker took, which is left as that worker has it.
    pub answers: Vec<LeaseAcknowledgement>,
    /// What the claim leased, empty when the round asked for none, or why it
    /// could not claim. A claim that fails leaves the settlements recorded.
    pub claims: Result<Vec<DeliveryClaim>, NotificationError>,
}
