//! Whether a copy of a local subscription is claimed at a registration before
//! it is sent to the registration's context source.
//!
//! A source may notify under a copy as soon as it holds it, and this broker
//! answers a notification under an identifier it does not know with 404, which
//! the source takes as final. So the copy is recorded, and its identifier
//! routed, before it is sent. The claim also keeps two placements of the same
//! copy from both sending one: the second finds the first recorded.

/// Whether a copy of a local subscription was claimed at a registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardedClaim {
    /// The copy is recorded under the identifier minted for it, and the
    /// placement that claimed it sends it.
    Claimed,
    /// Nothing was recorded, and the placement sends nothing: a copy of the
    /// subscription at the registration is already recorded, or the
    /// subscription or the registration no longer exists.
    Declined,
}
