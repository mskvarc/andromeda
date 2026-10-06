//! Sending one forwarded request and reading what it answered with, beside
//! the registration's clause 5.2.34 health members the attempt is recorded on.

use crate::{FederationService, outcome::TransportFailure};
use andromeda_http_headers::{
    link::{header::Link, relation_type::RelationType},
    ngsild_content_range::NgsiLdContentRange,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
    preference_applied::PreferenceApplied,
    warning::Warning,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::store::forwarding_outcome::ForwardingOutcome;
use andromeda_telemetry::federation_ops::record_upstream_status;
use chrono::Utc;
use iri_rs::IriRefBuf;
use std::{sync::Arc, time::Duration};
use tracing::warn;

/// What a forwarded request answered with, before the caller reads the body.
pub(crate) struct SentResponse {
    /// The HTTP status.
    pub(crate) status: reqwest::StatusCode,
    /// The IANA `Warning` header the source sent, if any.
    pub(crate) warning: Option<Warning>,
    /// The clause 6.3.17 warnings the source itself reported about the
    /// sources behind it, relayed to the client as part of this response.
    pub(crate) ngsild_warnings: Vec<NgsiLdWarning>,
    /// The specification version the source says its payload conforms to
    /// (clause 4.3.6.8 `Preference-Applied`), when it said.
    pub(crate) returned_version: Option<SpecVersion>,
    /// The `EntityMap` the source used (clause 5.5.14 `NGSILD-EntityMap`), when
    /// it returned one.
    pub(crate) entity_map: Option<Identifier>,
    /// The target of the source's `Link rel="next"` (clause 6.3.10), as the
    /// source wrote it: the reference to the page following this one, still to
    /// be resolved against the URL it answered.
    pub(crate) next_page: Option<IriRefBuf>,
    /// The temporal `Content-Range` of a 206 answer (clause 6.3.10), which says
    /// how far into each entity's history the source's answer reaches.
    pub(crate) partial_range: Option<NgsiLdContentRange>,
    /// The raw body.
    pub(crate) body: Vec<u8>,
}

impl FederationService {
    /// Send a request to `registration` and return its status, `Warning` and raw body.
    ///
    /// Every attempt is recorded on the registration's clause 5.2.9 health
    /// members and on the cooldown registry. A 404 is not a failure: clause
    /// 6.3.17 says a source answering 404 is not behaving abnormally, so it
    /// neither flips `status` nor opens a cooldown window.
    pub(crate) async fn send_request(
        &self,
        registration: &ContextSourceRegistration,
        request: reqwest::RequestBuilder,
    ) -> Result<SentResponse, TransportFailure> {
        let timeout = self.timeout_for(registration);
        let outcome = match request.timeout(timeout).send().await {
            Ok(response) => {
                let status = response.status();
                record_upstream_status(status.as_u16());
                let warning = headers::HeaderMapExt::typed_get::<Warning>(response.headers());
                let ngsild_warnings = headers::HeaderMapExt::typed_get::<NgsiLdWarningHeader>(response.headers())
                    .map(NgsiLdWarningHeader::into_warnings)
                    .unwrap_or_default();
                let returned_version = headers::HeaderMapExt::typed_get::<PreferenceApplied>(response.headers()).and_then(|applied| applied.applied_version());
                let entity_map = headers::HeaderMapExt::typed_get::<NgsiLdEntityMap>(response.headers()).map(NgsiLdEntityMap::into_inner);
                let next_page = headers::HeaderMapExt::typed_get::<Link>(response.headers())
                    .and_then(|link| link.find_by_rel(RelationType::Next).next().map(|value| value.link().clone())); // clone: the answer outlives the header map it was read from
                let partial_range = (status == reqwest::StatusCode::PARTIAL_CONTENT)
                    .then(|| headers::HeaderMapExt::typed_get::<NgsiLdContentRange>(response.headers()))
                    .flatten();
                match response.bytes().await {
                    Ok(body) => Ok(SentResponse {
                        status,
                        warning,
                        ngsild_warnings,
                        returned_version,
                        entity_map,
                        next_page,
                        partial_range,
                        body: body.to_vec(),
                    }),
                    Err(error) => Err(classify_transport_failure(error, timeout)),
                }
            }
            Err(error) => Err(classify_transport_failure(error, timeout)),
        };
        let health = match &outcome {
            Ok(sent) if sent.status.is_success() || sent.status == reqwest::StatusCode::NOT_FOUND => ForwardingOutcome::Succeeded,
            Ok(_) | Err(_) => ForwardingOutcome::Failed,
        };
        self.record_health(&registration.id, health).await;
        outcome
    }

    /// Record one attempt on the registration's health members (clause
    /// 5.2.34) before the response is handed back, so a client reading the
    /// registration right after the forwarded call observes the attempt.
    async fn record_health(&self, registration: &Identifier, outcome: ForwardingOutcome) {
        match outcome {
            ForwardingOutcome::Succeeded => self.cooldowns.record_success(registration),
            ForwardingOutcome::Failed => self.cooldowns.record_failure(registration),
        }
        if let Err(error) = self.csr_repo.record_forwarding_outcome(registration, outcome, Utc::now()).await {
            warn!(%registration, %error, "failed to record the forwarding outcome on the registration");
        }
    }
}

/// Sort a `reqwest` failure into the two clause 6.3.17 transport outcomes.
fn classify_transport_failure(error: reqwest::Error, timeout: Duration) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure::TimedOut { after: timeout }
    } else {
        TransportFailure::Undelivered(Arc::new(error))
    }
}
