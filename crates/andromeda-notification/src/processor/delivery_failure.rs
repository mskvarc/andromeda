//! Why a durably queued delivery did not complete, as recorded against it.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::error::SubscriptionError;
use thiserror::Error;

/// The reason a queued delivery is retried or failed, rendered into the
/// diagnostic stored with its disposition.
#[derive(Debug, Error)]
pub(super) enum DeliveryFailure {
    /// Every endpoint attempt that failed did so in a way a later attempt may overcome.
    #[error("{attempts} endpoint attempt(s) failed transiently")]
    TransientEndpoints {
        /// How many endpoint attempts failed transiently.
        attempts: usize,
    },
    /// An endpoint attempt failed in a way no later attempt can overcome.
    #[error("{attempts} endpoint attempt(s) failed permanently")]
    PermanentEndpoints {
        /// How many endpoint attempts failed permanently.
        attempts: usize,
    },
    /// The subscription could not be read back before its notification.
    #[error("subscription {subscription_id} could not be read back before its notification: {source}")]
    SubscriptionUnreadable {
        /// The subscription the delivery is for.
        subscription_id: Identifier,
        /// Why the store could not answer.
        #[source]
        source: SubscriptionError,
    },
}
