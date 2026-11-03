//! Whether a delivery pass is waiting on its round trip to the queue.
//!
//! A pass keeps at most one round in flight and gathers what becomes due
//! meanwhile for the round that follows, so a burst costs one round trip
//! rather than one per delivery.

/// Whether a round of the pass is in flight.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum RoundTrip {
    /// No round is in flight, so the next one goes out at once.
    #[default]
    Settled,
    /// A round was issued and has not come back yet.
    InFlight,
}
