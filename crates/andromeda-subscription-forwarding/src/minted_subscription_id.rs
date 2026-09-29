//! The identifier this broker mints for a subscription it hands to a peer.

use crate::error::SubscriptionForwardingError;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::NGSI_LD_URN_NID};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Mint an identifier for a subscription this broker hands to a peer: a
/// companion or a forwarded copy. Clause 5.2.12 lets the creator choose the
/// identifier, so the broker chooses one it can route back.
///
/// # Errors
///
/// Returns [`SubscriptionForwardingError::Internal`] when the minted
/// namespace-specific string is not a valid URN component or the minted URN is
/// past the identifier bound.
pub(crate) fn mint_subscription_id() -> Result<Identifier, SubscriptionForwardingError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:forwarded:{}", Uuid::new_v4()))
        .build()
        .map_err(|error| SubscriptionForwardingError::internal(error.to_string()))?;
    Identifier::try_from(urn).map_err(|error| SubscriptionForwardingError::internal(error.to_string()))
}
