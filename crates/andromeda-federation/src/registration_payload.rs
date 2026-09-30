//! What one registration receives: the context a forwarded request is expressed
//! against and the representation of its body (clauses 4.3.6.6, 6.3.19).
//!
//! The decision is read from the registration's admitted `contextSourceInfo`
//! and `jsonldContext` alone, so the query narrowing, the body and the
//! transport of one forwarded request all spell it from the same answer.

use crate::{
    FederationService,
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_payload::{ForwardedPayload, ItemBody, OutgoingBody, build_forwarded_payload, build_items_payload},
    forwarding_context::{BodyPresence, ForwardingDecision, decide, decide_for_items},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, registration::ContextSourceRegistration};

/// Read a registration's `contextSourceInfo`; stored registrations passed the
/// clause 4.3.6.6 restrictions at registration time, so a failure here is an
/// invariant violation rather than a request error.
pub(crate) fn context_source_info(registration: &ContextSourceRegistration) -> Result<ContextSourceInfo<'_>, FederationError> {
    let pairs = registration.access.context_source_info.as_deref().unwrap_or_default();
    ContextSourceInfo::try_from(pairs).map_err(|source| {
        FederationInvariant::ContextSourceInfo {
            registration: registration.id.clone(), // clone: the error owns the registration it names
            source,
        }
        .into()
    })
}

/// Decide how a request forwarded to `registration` is sent: its media type
/// and the one context it is expressed against (clauses 4.3.6.6, 6.3.19).
///
/// Every name a forwarded request carries and its body are spelled from
/// the same decision, so the query narrowing and the transport cannot
/// disagree about which context a request is written in.
///
/// # Errors
///
/// Returns [`FederationError::Internal`] when the registration's admitted
/// `contextSourceInfo` or `jsonldContext` no longer reads as admitted.
pub(crate) fn forwarding_decision(
    registration: &ContextSourceRegistration,
    inbound: &ContextSpec,
    presence: BodyPresence,
) -> Result<ForwardingDecision, FederationError> {
    let info = context_source_info(registration)?;
    decide(
        inbound,
        &info,
        registration.access.jsonld_context.as_ref().map(DereferenceableUri::url),
        presence,
    )
}

impl FederationService {
    /// Decide the representation one forwarded request carries for `registration`.
    pub(crate) async fn payload_for(
        &self,
        registration: &ContextSourceRegistration,
        inbound: &ContextSpec,
        body: Option<OutgoingBody<'_>>,
    ) -> Result<ForwardedPayload, FederationError> {
        let info = context_source_info(registration)?;
        let presence = match body {
            Some(_) => BodyPresence::Present,
            None => BodyPresence::Absent,
        };
        let decision = forwarding_decision(registration, inbound, presence)?;
        build_forwarded_payload(&self.engine, inbound, &decision, body, &info).await
    }

    /// Decide the representation of one forwarded batch array for
    /// `registration`, each entity read under the context it was submitted
    /// with.
    pub(crate) async fn payload_for_items(
        &self,
        registration: &ContextSourceRegistration,
        items: &[ItemBody<'_>],
    ) -> Result<ForwardedPayload, FederationError> {
        let info = context_source_info(registration)?;
        let decision = decide_for_items(
            items.iter().map(|item| item.context),
            &info,
            registration.access.jsonld_context.as_ref().map(DereferenceableUri::url),
        )?;
        build_items_payload(&self.engine, &decision, items, &info).await
    }
}
