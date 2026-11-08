//! Forwarding subscription operations to a registered context source
//! (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).
//!
//! A reduced copy of a local subscription is placed at, updated at, or removed
//! from a context source whose registration advertises the matching clause 4.20
//! operation. The representation rules are the ones every forwarded request
//! obeys (`forwarded_payload`), and the outcome is the typed write outcome the
//! provision paths use, so a source's answer is read the same way everywhere.

use crate::{
    FederationService,
    acted_on_answer::ActedOnAnswer,
    error::FederationError,
    forwarded_payload::OutgoingBody,
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    outcome::{ForwardedWriteOutcome, TransportFailure},
    remote_answer::RemoteAnswer,
    request::TriggeringRequest,
    subscription_forward_failure::SubscriptionForwardFailure,
    transport::ForwardedRequest,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription_request::SubscriptionRequest};
use serde::Deserialize;
use tracing::debug;

/// One subscription operation to forward to a context source.
pub enum SubscriptionForward<'a> {
    /// `POST /subscriptions` with the reduced copy (clause 5.8.1.4).
    Create {
        /// The reduced subscription.
        body: &'a SubscriptionRequest,
        /// The context the subscription's terms are expressed in.
        context: &'a ContextSpec,
    },
    /// `PATCH /subscriptions/{id}` with the reduced copy (clause 5.8.2.4).
    Update {
        /// The subscription the source assigned when the copy was created.
        remote_id: &'a Identifier,
        /// The reduced subscription fragment.
        body: &'a SubscriptionRequest,
        /// The context the fragment's terms are expressed in.
        context: &'a ContextSpec,
    },
    /// `DELETE /subscriptions/{id}` (clause 5.8.5.4).
    Delete {
        /// The subscription the source assigned when the copy was created.
        remote_id: &'a Identifier,
    },
}

impl SubscriptionForward<'_> {
    /// The clause 4.20 operation the registration has to advertise.
    const fn operation(&self) -> ForwardingOperation {
        match self {
            SubscriptionForward::Create { .. } => ForwardingOperation::CreateSubscription,
            SubscriptionForward::Update { .. } => ForwardingOperation::UpdateSubscription,
            SubscriptionForward::Delete { .. } => ForwardingOperation::DeleteSubscription,
        }
    }
}

/// Whether a registration advertises the operation a forward needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionForwardAdmission {
    /// The registration advertises the operation; the forward was attempted.
    Forwarded,
    /// The registration does not advertise the operation; nothing was sent.
    NotAdvertised,
}

/// The identity a source answers a created subscription with, when it echoes
/// the document (clause 5.2.12 `id`).
#[derive(Deserialize)]
struct AssignedIdentity {
    /// The subscription identifier the source holds the copy under.
    id: Identifier,
}

/// What one subscription forward produced.
pub struct ForwardedSubscription {
    /// Whether the source was contacted at all.
    pub admission: SubscriptionForwardAdmission,
    /// The source's answer, when it was contacted.
    pub outcome: Option<ForwardedWriteOutcome>,
}

impl ForwardedSubscription {
    /// The subscription identifier the source assigned on a create, read from
    /// its `Location` is not available here, so from the 201 response body's
    /// `id` when the source echoes the document, else the one the request named.
    #[must_use]
    pub fn assigned_id(&self, requested: &Identifier) -> Identifier {
        match &self.outcome {
            Some(ForwardedWriteOutcome::Completed { body, .. }) => body.decoded::<AssignedIdentity>().map_or_else(|| requested.clone(), |assigned| assigned.id), // clone: the caller keeps the id it minted
            Some(
                ForwardedWriteOutcome::RemoteHttpFailure { .. }
                | ForwardedWriteOutcome::Transport(_)
                | ForwardedWriteOutcome::LoopDetected
                | ForwardedWriteOutcome::NotAdvertised
                | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised,
            )
            | None => requested.clone(),
        }
    }

    /// Why the source did not complete the operation; `None` when it did.
    #[must_use]
    pub fn failure(&self) -> Option<SubscriptionForwardFailure> {
        match &self.outcome {
            Some(ForwardedWriteOutcome::Completed { .. }) => None,
            Some(ForwardedWriteOutcome::RemoteHttpFailure { status, body }) => Some(SubscriptionForwardFailure::Answered(RemoteAnswer {
                status: *status,
                problem: body.problem(),
            })),
            Some(ForwardedWriteOutcome::Transport(failure)) => Some(SubscriptionForwardFailure::Transport(failure.clone())), // clone: the failure outlives the outcome it reports
            Some(ForwardedWriteOutcome::LoopDetected) => Some(SubscriptionForwardFailure::LoopDetected),
            Some(ForwardedWriteOutcome::NotAdvertised | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised) | None => {
                Some(SubscriptionForwardFailure::NotAdvertised)
            }
        }
    }
}

impl FederationService {
    /// Forward one subscription operation to `registration`, when it
    /// advertises the operation (clause 5.8.1.4: "if the Context Source
    /// Registration indicates support for ...").
    pub async fn forward_subscription(&self, registration: &ContextSourceRegistration, forward: SubscriptionForward<'_>) -> ForwardedSubscription {
        if !forward.operation().matches(registration) {
            return ForwardedSubscription {
                admission: SubscriptionForwardAdmission::NotAdvertised,
                outcome: None,
            };
        }
        let outcome = self.send_subscription_forward(registration, forward).await;
        ForwardedSubscription {
            admission: SubscriptionForwardAdmission::Forwarded,
            outcome: Some(outcome),
        }
    }

    /// Send the forward and read the source's answer.
    async fn send_subscription_forward(&self, registration: &ContextSourceRegistration, forward: SubscriptionForward<'_>) -> ForwardedWriteOutcome {
        let unreachable = |error: FederationError| ForwardedWriteOutcome::Transport(TransportFailure::unprepared(error));
        // A subscription forward originates here rather than continuing an
        // inbound request, so there is no `Via` chain to extend; the
        // registration's own alias still cannot be one this broker answers for.
        let triggering = TriggeringRequest::default();
        if self.registration_loops(registration, triggering.via) {
            return ForwardedWriteOutcome::LoopDetected;
        }
        if let Some(failure) = self.cooldowns.holding_back(registration) {
            return ForwardedWriteOutcome::Transport(failure);
        }
        let (method, path, body, context) = match &forward {
            SubscriptionForward::Create { body, context } => (reqwest::Method::POST, vec!["subscriptions"], Some(*body), *context),
            SubscriptionForward::Update { remote_id, body, context } => {
                (reqwest::Method::PATCH, vec!["subscriptions", remote_id.as_str()], Some(*body), *context)
            }
            SubscriptionForward::Delete { remote_id } => (reqwest::Method::DELETE, vec!["subscriptions", remote_id.as_str()], None, &CORE_CONTEXT),
        };
        let url = match forwarding_url(registration.access.endpoint.url(), &path, None) {
            Ok(url) => url,
            Err(error) => return unreachable(error),
        };
        debug!("CS forwarding subscription {} {}", method, url);
        // A reduced subscription is respelled member by member, so its shape
        // survives a configured forwarding context (clause 4.3.6.6).
        let payload = match self.payload_for(registration, context, body.map(OutgoingBody::Subscription)).await {
            Ok(payload) => payload,
            Err(error) => return unreachable(error),
        };
        let request = match self.build_request(ForwardedRequest {
            method,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map: None,
        }) {
            Ok(request) => request,
            Err(error) => return unreachable(error),
        };
        match self.send_request(registration, request, ActedOnAnswer::NoErrorAnswer).await {
            Ok(sent) => ForwardedWriteOutcome::of_sent(&registration.id, &sent),
            Err(failure) => ForwardedWriteOutcome::Transport(failure),
        }
    }
}

/// The context a body-less forward is expressed against.
static CORE_CONTEXT: ContextSpec = ContextSpec::core_only();

#[cfg(test)]
mod tests {
    use crate::{
        answer_body::AnswerBody,
        outcome::ForwardedWriteOutcome,
        subscription::{ForwardedSubscription, SubscriptionForwardAdmission},
    };
    use std::sync::Arc;

    fn forwarded(outcome: ForwardedWriteOutcome) -> ForwardedSubscription {
        ForwardedSubscription {
            admission: SubscriptionForwardAdmission::Forwarded,
            outcome: Some(outcome),
        }
    }

    #[test]
    fn a_completed_forward_reports_no_failure() {
        let completed = forwarded(ForwardedWriteOutcome::Completed {
            status: 201,
            body: AnswerBody::Absent,
        });
        assert!(completed.failure().is_none());
    }

    /// Clause 6.3.17: a refused copy is reported with the problem detail the
    /// source answered, which names what it refused.
    #[test]
    fn a_refused_forward_reports_the_status_and_the_problem_detail_of_the_source() {
        let refused = forwarded(ForwardedWriteOutcome::RemoteHttpFailure {
            status: 400,
            body: AnswerBody::Text(Arc::from(
                br#"{"type":"https://uri.etsi.org/ngsi-ld/errors/BadRequestData","detail":"entities or watchedAttributes must be present"}"#.as_slice(),
            )),
        });
        pretty_assertions::assert_eq!(
            refused.failure().map(|failure| failure.to_string()).as_deref(),
            Some("context source answered HTTP 400: entities or watchedAttributes must be present")
        );
    }

    #[test]
    fn a_forward_never_sent_reports_the_operation_as_not_advertised() {
        let unsent = ForwardedSubscription {
            admission: SubscriptionForwardAdmission::NotAdvertised,
            outcome: None,
        };
        pretty_assertions::assert_eq!(
            unsent.failure().map(|failure| failure.to_string()).as_deref(),
            Some("the registration does not advertise the operation")
        );
    }
}
