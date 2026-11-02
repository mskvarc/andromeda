//! The order a claim takes due deliveries in.

use std::num::NonZeroU32;

/// Which due deliveries a claim takes first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryClaimOrder {
    /// The deliveries that became claimable first, whatever their
    /// Subscription: one range scan as deep as the claim.
    Queue,
    /// The deliveries of the Subscriptions with the fewest deliveries leased
    /// to the claiming worker first, then those that became claimable first,
    /// leaving out the Subscriptions the ceiling excludes.
    FewestInFlight(InFlightCeiling),
}

/// How many deliveries one Subscription may have leased to the claiming
/// worker before a claim passes it over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InFlightCeiling {
    /// A Subscription with this many or more is passed over, so the slots it
    /// does not hold stay for the others. A claim under a ceiling that comes
    /// back short says nothing about what the queue still holds.
    Below(NonZeroU32),
    /// No Subscription is passed over, so a Subscription with no other beside
    /// it uses every slot.
    Lifted,
}
