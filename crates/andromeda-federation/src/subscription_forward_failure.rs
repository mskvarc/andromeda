//! Why a context source did not complete a forwarded subscription operation
//! (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).

use crate::{outcome::TransportFailure, remote_answer::RemoteAnswer, remote_effect::RemoteEffect};
use thiserror::Error;

/// Why one subscription forward did not complete at the source.
#[derive(Debug, Clone, Error)]
pub enum SubscriptionForwardFailure {
    /// The source answered with an error status, and with the problem detail
    /// that names the member it refused when it sent one.
    #[error(transparent)]
    Answered(RemoteAnswer),
    /// No answer was obtained from the source.
    #[error(transparent)]
    Transport(TransportFailure),
    /// The registration loops back into this broker, so the source was not
    /// contacted (clauses 4.3.6.4 and 6.3.17).
    #[error("the registration loops back into this broker")]
    LoopDetected,
    /// The registration does not advertise the operation, so the source was
    /// not contacted.
    #[error("the registration does not advertise the operation")]
    NotAdvertised,
}

impl SubscriptionForwardFailure {
    /// What the forward may have left at the source. An error status is the
    /// source saying it did not carry the operation out.
    #[must_use]
    pub fn remote_effect(&self) -> RemoteEffect {
        match self {
            SubscriptionForwardFailure::Transport(failure) => failure.remote_effect(),
            SubscriptionForwardFailure::Answered(_) | SubscriptionForwardFailure::LoopDetected | SubscriptionForwardFailure::NotAdvertised => {
                RemoteEffect::Untouched
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        outcome::TransportFailure,
        remote_answer::RemoteAnswer,
        remote_effect::RemoteEffect,
        subscription_forward_failure::SubscriptionForwardFailure,
    };
    use std::{sync::Arc, time::Duration};

    #[test]
    fn a_forward_that_got_no_answer_in_time_may_have_been_carried_out() {
        let timed_out = SubscriptionForwardFailure::Transport(TransportFailure::TimedOut { after: Duration::from_secs(5) });
        pretty_assertions::assert_eq!(timed_out.remote_effect(), RemoteEffect::Possible);
    }

    #[test]
    fn a_forward_the_source_refused_or_never_received_left_nothing_there() {
        let untouched = [
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 400, problem: None }),
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 500, problem: None }),
            SubscriptionForwardFailure::Transport(TransportFailure::CoolingDown {
                remaining: Duration::from_secs(1),
            }),
            SubscriptionForwardFailure::Transport(TransportFailure::Unprepared(Arc::new(FederationError::LoopDetected {
                registration: "urn:ngsi-ld:ContextSourceRegistration:b".parse().expect("a valid identifier"),
            }))),
            SubscriptionForwardFailure::LoopDetected,
            SubscriptionForwardFailure::NotAdvertised,
        ];
        for failure in untouched {
            pretty_assertions::assert_eq!(failure.remote_effect(), RemoteEffect::Untouched, "{failure}");
        }
    }

    /// Nothing listens on the discard port of the loopback address, so the
    /// connection is refused before any request is written.
    #[tokio::test]
    async fn a_forward_whose_connection_was_refused_left_nothing_there() {
        let error = reqwest::Client::new()
            .post("http://127.0.0.1:9/ngsi-ld/v1/subscriptions")
            .send()
            .await
            .expect_err("nothing listens on the discard port");
        let refused = SubscriptionForwardFailure::Transport(TransportFailure::Undelivered(Arc::new(error)));
        pretty_assertions::assert_eq!(refused.remote_effect(), RemoteEffect::Untouched);
    }
}
