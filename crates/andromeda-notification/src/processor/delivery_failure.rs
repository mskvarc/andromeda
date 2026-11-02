//! Why a durably queued delivery did not complete, as recorded against it.

use crate::processor::attempt_failure::FailedAttempts;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::error::SubscriptionError;
use thiserror::Error;

/// The reason a queued delivery is retried or failed, rendered into the
/// diagnostic stored with its disposition.
///
/// An endpoint failure is stored with the cause of its first attempt, the text
/// the dispatcher logged for it, so the stored diagnostic says why the
/// notification failed and not only that it did.
#[derive(Debug, Error)]
pub(super) enum DeliveryFailure {
    /// Every endpoint attempt that failed did so in a way a later attempt may overcome.
    #[error("{} endpoint attempt(s) failed transiently, the first because {}", .0.attempts, .0.first)]
    TransientEndpoints(FailedAttempts),
    /// An endpoint attempt failed in a way no later attempt can overcome.
    #[error("{} endpoint attempt(s) failed permanently, the first because {}", .0.attempts, .0.first)]
    PermanentEndpoints(FailedAttempts),
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

#[cfg(test)]
mod tests {
    use crate::processor::{
        attempt_failure::{FailedAttempts, NotificationAttemptFailure},
        delivery_failure::DeliveryFailure,
    };
    use andromeda_delivery::{endpoint_failure::EndpointFailure, error::TransportError, oversized_notification::OversizedNotification};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_permanent_failure_carries_the_refusal_text_the_dispatcher_logs() {
        let refusal = OversizedNotification::MqttServerAnnounced {
            size: 8_389_548,
            announced: 2_000_000,
            configured: 134_217_728,
        };
        let logged = TransportError::from(refusal).to_string();
        let first = NotificationAttemptFailure::Endpoint(EndpointFailure::Transport(TransportError::from(refusal)));
        let stored = DeliveryFailure::PermanentEndpoints(FailedAttempts::new(first)).to_string();
        assert_eq!(stored, format!("1 endpoint attempt(s) failed permanently, the first because {logged}"));
    }

    #[test]
    fn a_transient_failure_counts_every_attempt_and_names_the_first_cause() {
        let mut tally = None;
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::ContextUnloadable);
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::ContextUnloadable);
        let stored = tally.map(|failed| DeliveryFailure::TransientEndpoints(failed).to_string());
        assert_eq!(
            stored.as_deref(),
            Some("2 endpoint attempt(s) failed transiently, the first because the jsonldContext of the subscription could not be loaded")
        );
    }
}
