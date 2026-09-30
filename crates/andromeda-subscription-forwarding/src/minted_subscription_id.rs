//! The identifier this broker mints for a subscription it hands to a peer.

use crate::{error::SubscriptionForwardingError, forwarding_invariant::ForwardingInvariant};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::SubscriptionId};

/// Mint an identifier for a subscription this broker hands to a peer: a
/// companion or a forwarded copy, `urn:ngsi-ld:Subscription:forwarded-<uuid>`.
/// Clause 5.2.12 lets the creator choose the identifier. The broker routes a
/// peer's notifications back by its route and mapping records, keyed by the
/// whole identifier, never by parsing the `forwarded-` marker, which only tells
/// a human reading the peer's subscription list where the copy came from.
///
/// # Errors
///
/// Returns [`ForwardingInvariant::MintedSubscriptionId`] when the minted
/// identifier does not parse back, which a hyphenated UUID under the
/// `Subscription` kind never fails to do.
pub(crate) fn mint_subscription_id() -> Result<Identifier, SubscriptionForwardingError> {
    SubscriptionId::mint_forwarded()
        .map(Identifier::from)
        .map_err(|error| ForwardingInvariant::MintedSubscriptionId(error).into())
}
