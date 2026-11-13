//! Whether a copy of a local subscription is claimed at a registration before
//! it is sent to the registration's context source, and the claims whose
//! placement was abandoned before the source answered.
//!
//! A source may notify under a copy as soon as it holds it, and this broker
//! answers a notification under an identifier it does not know with 404, which
//! the source takes as final. So the copy is recorded, and its identifier
//! routed, before it is sent. The claim also keeps two placements of the same
//! copy from both sending one, since the second finds the first recorded.
//!
//! A claim holds a lease until the source's answer settles it. A placement cut
//! off before that (by the request deadline, a shutdown or a crash) leaves its
//! lease to run out, and the claim is then taken over and the copy sent again
//! under the identifier it was recorded with. A source holding that copy
//! already answers the second creation with `AlreadyExists`, so it never holds
//! two. Since an earlier send of a taken-over claim may have reached the
//! source, only the source refusing the copy as sent releases it. A send again
//! the source answered with an error status that depends on the moment it
//! came, or that never reached the source, leaves the claim to its lease, to
//! be tried once more.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Whether a copy of a local subscription was claimed at a registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardedClaim {
    /// The copy is recorded under a lease, and the placement that claimed it
    /// sends it under `remote`.
    Claimed {
        /// The identifier the copy is sent under. That is the identifier
        /// minted for the claim when it was recorded with the copy, and the
        /// identifier recorded when it took over an abandoned claim.
        remote: Identifier,
        /// How the claim came to the placement.
        origin: ClaimOrigin,
    },
    /// Nothing was recorded, and the placement sends nothing. A copy of the
    /// subscription at the registration is settled or still under a lease, or
    /// the subscription or the registration no longer exists.
    Declined,
}

/// How a placement came to hold its claim, which tells whether an earlier
/// send of the same copy may have reached the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOrigin {
    /// The claim was recorded with the copy, so nothing was sent before it and
    /// a send that never reached the source leaves nothing there.
    Recorded,
    /// The claim was taken over from a placement whose lease ran out, whose
    /// send may have reached the source. A send the source answers with an
    /// error status that depends on the moment it came, or that never reaches
    /// the source, learns nothing of it, so the claim stays to its lease.
    TakenOver,
}

/// A claim whose lease ran out before the source's answer settled it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleClaim {
    /// The local subscription the copy reduces.
    pub local: Identifier,
    /// The registration whose context source the copy was sent to.
    pub registration: Identifier,
    /// The identifier the copy was recorded with.
    pub remote: Identifier,
}
