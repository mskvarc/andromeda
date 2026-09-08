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
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    outcome::{ForwardedWriteOutcome, TransportFailure},
    request::TriggeringRequest,
    transport::ForwardedRequest,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::Value;
use tracing::debug;

/// One subscription operation to forward to a context source.
pub enum SubscriptionForward<'a> {
    /// `POST /subscriptions` with the reduced copy (clause 5.8.1.4).
    Create {
        /// The reduced subscription document, without an inline `@context`.
        body: &'a [u8],
        /// The context the document's terms are expressed in.
        context: &'a ContextSpec,
    },
    /// `PATCH /subscriptions/{id}` with the reduced copy (clause 5.8.2.4).
    Update {
        /// The subscription the source assigned when the copy was created.
        remote_id: &'a Uri,
        /// The reduced subscription fragment, without an inline `@context`.
        body: &'a [u8],
        /// The context the fragment's terms are expressed in.
        context: &'a ContextSpec,
    },
    /// `DELETE /subscriptions/{id}` (clause 5.8.5.4).
    Delete {
        /// The subscription the source assigned when the copy was created.
        remote_id: &'a Uri,
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
    pub fn assigned_id(&self, requested: &Uri) -> Uri {
        match &self.outcome {
            Some(ForwardedWriteOutcome::Completed {
                body: Some(Value::Object(object)),
                ..
            }) => object
                .get("id")
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse().ok())
                .unwrap_or_else(|| requested.clone()), // clone: the caller keeps the id it minted
            Some(
                ForwardedWriteOutcome::Completed { .. }
                | ForwardedWriteOutcome::RemoteHttpFailure { .. }
                | ForwardedWriteOutcome::Transport(_)
                | ForwardedWriteOutcome::LoopDetected
                | ForwardedWriteOutcome::NotAdvertised
                | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised,
            )
            | None => requested.clone(), // clone: same
        }
    }

    /// Whether the source completed the operation.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self.outcome, Some(ForwardedWriteOutcome::Completed { .. }))
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
        let unreachable = |detail: String| ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { detail });
        // A subscription forward originates here rather than continuing an
        // inbound request, so there is no `Via` chain to extend; the
        // registration's own alias still cannot be one this broker answers for.
        let triggering = TriggeringRequest::default();
        if self.registration_loops(registration, triggering.via) {
            return ForwardedWriteOutcome::LoopDetected;
        }
        let (method, path, body, context) = match &forward {
            SubscriptionForward::Create { body, context } => (reqwest::Method::POST, vec!["subscriptions"], Some(*body), *context),
            SubscriptionForward::Update { remote_id, body, context } => {
                (reqwest::Method::PATCH, vec!["subscriptions", remote_id.as_str()], Some(*body), *context)
            }
            SubscriptionForward::Delete { remote_id } => (reqwest::Method::DELETE, vec!["subscriptions", remote_id.as_str()], None, &CORE_CONTEXT),
        };
        let url = match forwarding_url(&registration.access.endpoint, &path, "") {
            Ok(url) => url,
            Err(error) => return unreachable(error.to_string()),
        };
        debug!("CS forwarding subscription {} {}", method, url);
        let payload = match self.payload_for(registration, context, body).await {
            Ok(payload) => payload,
            Err(error) => return unreachable(error.to_string()),
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
            Err(error) => return unreachable(error.to_string()),
        };
        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardedWriteOutcome::Transport(failure),
        };
        let response_body = if sent.body.is_empty() {
            None
        } else {
            serde_json::from_slice::<Value>(&sent.body).ok()
        };
        let status = sent.status.as_u16();
        if sent.status.is_success() {
            ForwardedWriteOutcome::Completed { status, body: response_body }
        } else {
            ForwardedWriteOutcome::RemoteHttpFailure { status, body: response_body }
        }
    }
}

/// The context a body-less forward is expressed against.
static CORE_CONTEXT: ContextSpec = ContextSpec::core_only();
