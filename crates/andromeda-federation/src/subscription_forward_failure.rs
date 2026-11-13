//! Why a context source did not complete a forwarded subscription operation
//! (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).

use crate::{outcome::TransportFailure, remote_answer::RemoteAnswer, remote_effect::RemoteEffect};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use http::StatusCode;
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
    /// source saying it did not carry the operation out, except a 409 that
    /// names `AlreadyExists`, or no problem type at all, which says the source
    /// holds what the operation creates already, and a 502 or 504 that does
    /// not name `LdContextNotAvailable`, a gateway saying the source behind it
    /// gave no valid answer in time, which leaves the operation as possible as
    /// no answer at all does.
    #[must_use]
    pub fn remote_effect(&self) -> RemoteEffect {
        match self {
            SubscriptionForwardFailure::Transport(failure) => failure.remote_effect(),
            SubscriptionForwardFailure::Answered(answer) => answered_effect(answer),
            SubscriptionForwardFailure::LoopDetected | SubscriptionForwardFailure::NotAdvertised => RemoteEffect::Untouched,
        }
    }
}

/// What the error status `answer` left at the source.
///
/// A 409 is the status a source answers the creation of something it already
/// holds with (`AlreadyExists`, clause 5.5.2), and a 409 of another problem
/// type, `Conflict` say, refused the operation for a reason of its own, so it
/// left nothing there. A gateway answers 502 when the source behind it gave an
/// invalid answer and 504 when it gave none in time, after passing the
/// operation on (RFC 9110 sections 15.6.3 and 15.6.5, clause 6.3.17). The one
/// error type Table 6.3.2-1 answers with 504 is `LdContextNotAvailable`, the
/// source refusing the operation because it could not retrieve its `@context`.
fn answered_effect(answer: &RemoteAnswer) -> RemoteEffect {
    let already_exists = BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::AlreadyExists));
    let context_not_available = BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::LdContextNotAvailable));
    let names = |kind: &BatchErrorKind| answer.problem.as_ref().is_some_and(|problem| problem.error_type == *kind);
    match StatusCode::from_u16(answer.status) {
        Ok(StatusCode::CONFLICT) if answer.problem.is_none() || names(&already_exists) => RemoteEffect::Present,
        Ok(StatusCode::GATEWAY_TIMEOUT) if names(&context_not_available) => RemoteEffect::Untouched,
        Ok(StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT) => RemoteEffect::Possible,
        Ok(_) | Err(_) => RemoteEffect::Untouched,
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
    use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
    use serde_json::json;
    use std::{sync::Arc, time::Duration};

    /// An answer of `status` carrying a problem document of `problem_type`.
    fn answered(status: u16, problem_type: &str) -> SubscriptionForwardFailure {
        let problem: BatchProblemDetail = serde_json::from_value(json!({"type": problem_type, "detail": "the forward failed"})).expect("a problem detail");
        SubscriptionForwardFailure::Answered(RemoteAnswer {
            status,
            problem: Some(problem),
        })
    }

    /// `AlreadyExists` (clause 5.5.2) is the source saying it holds what the
    /// creation would create, which is how a copy sent again under the
    /// identifier it already holds is answered.
    #[test]
    fn a_409_naming_already_exists_found_what_the_forward_creates_present() {
        let already_exists = answered(409, "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");
        pretty_assertions::assert_eq!(already_exists.remote_effect(), RemoteEffect::Present);
    }

    /// The status alone is the one clause 6.3.2 gives `AlreadyExists`, so a
    /// source that sends no problem document is read the same way.
    #[test]
    fn a_409_without_a_problem_document_found_what_the_forward_creates_present() {
        let bare = SubscriptionForwardFailure::Answered(RemoteAnswer { status: 409, problem: None });
        pretty_assertions::assert_eq!(bare.remote_effect(), RemoteEffect::Present);
    }

    /// `Conflict` refuses the operation for a reason of its own, so the source
    /// holds nothing of it.
    #[test]
    fn a_409_naming_another_problem_type_left_nothing_there() {
        let conflict = answered(409, "https://uri.etsi.org/ngsi-ld/errors/Conflict");
        pretty_assertions::assert_eq!(conflict.remote_effect(), RemoteEffect::Untouched);
    }

    #[test]
    fn a_forward_that_got_no_answer_in_time_may_have_been_carried_out() {
        let timed_out = SubscriptionForwardFailure::Transport(TransportFailure::TimedOut { after: Duration::from_secs(5) });
        pretty_assertions::assert_eq!(timed_out.remote_effect(), RemoteEffect::Possible);
    }

    /// A gateway that passed the forward on answers 502 for an invalid answer
    /// of the source behind it and 504 for none in time (RFC 9110 sections
    /// 15.6.3 and 15.6.5, clause 6.3.17), so the source may have carried it
    /// out.
    #[test]
    fn a_forward_a_gateway_got_no_valid_answer_to_may_have_been_carried_out() {
        let gateways = [
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 502, problem: None }),
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 504, problem: None }),
            answered(504, "about:blank"),
        ];
        for gateway in gateways {
            pretty_assertions::assert_eq!(gateway.remote_effect(), RemoteEffect::Possible, "{gateway}");
        }
    }

    /// Table 6.3.2-1 answers `LdContextNotAvailable` with 504, which is the
    /// source refusing the forward, not a gateway passing it on.
    #[test]
    fn a_504_naming_ld_context_not_available_left_nothing_there() {
        let context_not_available = answered(504, "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
        pretty_assertions::assert_eq!(context_not_available.remote_effect(), RemoteEffect::Untouched);
    }

    #[test]
    fn a_forward_the_source_refused_or_never_received_left_nothing_there() {
        let untouched = [
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 400, problem: None }),
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 500, problem: None }),
            SubscriptionForwardFailure::Answered(RemoteAnswer { status: 503, problem: None }),
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
