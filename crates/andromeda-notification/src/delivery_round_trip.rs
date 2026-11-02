//! Whether a delivery pass is waiting on one of its requests to the queue.
//!
//! A pass keeps at most one claim and at most one acknowledgement in flight,
//! and gathers what becomes due meanwhile for the request that follows, so a
//! burst costs one round trip per request kind rather than one per delivery.

/// Whether a request of the pass is in flight.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum RoundTrip {
    /// No request is in flight, so the next one goes out at once.
    #[default]
    Settled,
    /// A request was issued and has not come back yet.
    InFlight,
}
