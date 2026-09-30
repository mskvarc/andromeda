//! Reads forwarded to one registration: sending the GET or the POST batch
//! query, interpreting the answer (clause 6.3.17) and reading it back in the
//! client's terms (clause 4.3.6.6).

use crate::{
    FederationService,
    SentResponse,
    answer_vocabulary::{AnswerRewording, AnswerVocabulary},
    cache::{CacheKey, Cacheability, ForwardedGetCache},
    error::FederationError,
    forwarded_payload::OutgoingBody,
    forwarding_context::{BodyPresence, ForwardingDecision},
    outcome::{ForwardGetOutcome, ForwardedResponse, TransportFailure},
    registration_payload::forwarding_decision,
    request::TriggeringRequest,
    source_continuation::SourceContinuation,
    transport::ForwardedRequest,
};
use andromeda_http_headers::{ngsild_warning::NgsiLdWarning, warning::Warning};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, warn};
use typed_builder::TypedBuilder;
use url::Url;

/// One read forwarded to one registration: who is asked, under which inbound
/// context, and how its answer is read back.
#[derive(Clone, Copy, TypedBuilder)]
pub(crate) struct SourceRead<'a> {
    /// The registration being contacted.
    pub(crate) registration: &'a ContextSourceRegistration,
    /// The context the triggering request resolved to.
    pub(crate) inbound: &'a ContextSpec,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `EntityMap` the registration's context source created for this
    /// query, sent back to it in `NGSILD-EntityMap` (clause 5.5.14); none
    /// outside an Entity Map.
    #[builder(default = None)]
    pub(crate) linked_map: Option<&'a Identifier>,
    /// What kind of answer the read returns, which decides where its names sit.
    pub(crate) vocabulary: AnswerVocabulary,
}

impl FederationService {
    /// Read a source's answer back in the client's terms when the request was
    /// written in another context (clause 4.3.6.6).
    async fn read_back(
        &self,
        outcome: ForwardGetOutcome,
        inbound: &ContextSpec,
        decision: &ForwardingDecision,
        vocabulary: AnswerVocabulary,
    ) -> ForwardGetOutcome {
        let ForwardGetOutcome::Success(mut response) = outcome else {
            return outcome;
        };
        let forwarding = decision.context.naming(inbound);
        let rewording = AnswerRewording::new(&self.engine, &forwarding, inbound);
        match rewording.reword(response.body, vocabulary, decision.context.rewording()).await {
            Ok(body) => {
                response.body = body;
                ForwardGetOutcome::Success(response)
            }
            Err(error) => ForwardGetOutcome::UnreadableNames(Arc::new(error)),
        }
    }

    /// Make a GET request to a context source endpoint, and read its answer
    /// back in the client's terms.
    pub(crate) async fn forward_get(&self, read: SourceRead<'_>, url: Result<Url, FederationError>, cacheability: Cacheability) -> ForwardGetOutcome {
        let SourceRead {
            registration,
            inbound,
            triggering,
            linked_map,
            vocabulary,
        } = read;
        if self.registration_loops(registration, triggering.via) {
            return ForwardGetOutcome::LoopDetected;
        }
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let decision = match forwarding_decision(registration, inbound, BodyPresence::Absent) {
            Ok(decision) => decision,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let payload = match self.payload_for(registration, inbound, None).await {
            Ok(payload) => payload,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let cache_duration = match cacheability {
            Cacheability::Cacheable => ForwardedGetCache::duration_of(registration),
            Cacheability::Uncacheable => None,
        };
        // The cache holds the answer as the source wrote it, in the forwarding
        // context the key names; each reuse reads it back in its own client's
        // terms.
        let cache_key = cache_duration.map(|_| CacheKey::new(registration, &url, payload.link_context.as_ref()));
        if let Some(key) = &cache_key
            && let Some(response) = self.cache.lookup(key)
        {
            return self.read_back(ForwardGetOutcome::Success(response), inbound, &decision, vocabulary).await;
        }
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardGetOutcome::Transport(TransportFailure::CoolingDown { remaining });
        }
        debug!("CS forwarding GET {}", url);

        let answered = url.clone(); // clone: the next-page pointer resolves against the URL the request consumes
        let request = match self.build_request(ForwardedRequest {
            method: reqwest::Method::GET,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardGetOutcome::Transport(failure),
        };

        let outcome = interpret_read_response(registration, &answered, sent);
        if let ForwardGetOutcome::Success(response) = &outcome
            && let (Some(key), Some(duration)) = (cache_key, cache_duration)
        {
            self.cache.store(key, response, duration);
        }
        self.read_back(outcome, inbound, &decision, vocabulary).await
    }

    /// Make a POST batch query (`/entityOperations/query`) to a context source
    /// (clause 6.23), sent when a registration advertises `queryBatch` but not a
    /// direct retrieve/query (clause 4.3.6.1). The batch query is not among the
    /// cacheable GET resources of clause 6.3.17, so no cache is consulted, and its
    /// array answer is read exactly as a forwarded query's (clause 5.7.2.4).
    ///
    /// `body` is the `Query` this broker wrote in the registration's forwarding
    /// context ([`crate::forwarded_names`]), so it is sent as written.
    pub(crate) async fn forward_post_query(&self, read: SourceRead<'_>, url: Result<Url, FederationError>, body: &[u8]) -> ForwardGetOutcome {
        let SourceRead {
            registration,
            inbound,
            triggering,
            linked_map,
            vocabulary,
        } = read;
        if self.registration_loops(registration, triggering.via) {
            return ForwardGetOutcome::LoopDetected;
        }
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let decision = match forwarding_decision(registration, inbound, BodyPresence::Present) {
            Ok(decision) => decision,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let payload = match self.payload_for(registration, inbound, Some(OutgoingBody::Expressed(body))).await {
            Ok(payload) => payload,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardGetOutcome::Transport(TransportFailure::CoolingDown { remaining });
        }
        debug!("CS forwarding POST {}", url);

        let answered = url.clone(); // clone: the next-page pointer resolves against the URL the request consumes
        let request = match self.build_request(ForwardedRequest {
            method: reqwest::Method::POST,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardGetOutcome::Transport(failure),
        };
        let outcome = interpret_read_response(registration, &answered, sent);
        self.read_back(outcome, inbound, &decision, vocabulary).await
    }
}

/// Read a forwarded read response into its clause 6.3.17 outcome: a 404 that
/// contributes nothing, an error status, a stale/relayed warning set, or the
/// parsed entities. The GET and the POST batch-query bindings answer the same
/// way (clause 5.7.2.4), so both interpret their response here. `answered` is
/// the URL the request was sent to, which the source's `next` pointer resolves
/// against (clause 6.3.10).
fn interpret_read_response(registration: &ContextSourceRegistration, answered: &Url, sent: SentResponse) -> ForwardGetOutcome {
    let registration_id = &registration.id;
    if sent.status == reqwest::StatusCode::NOT_FOUND {
        return ForwardGetOutcome::NotFound(sent.ngsild_warnings);
    }
    if !sent.status.is_success() {
        return ForwardGetOutcome::ErrorStatus(sent.status.as_u16());
    }
    if let Some(version) = sent.returned_version {
        // Clause 4.3.6.8: the version the source says its payload conforms
        // to is recorded; the payload is read as the entity data it is.
        debug!(registration = %registration_id, %version, "context source answered with its conformant version");
    }
    // Propagate IANA Warning 110 (Response is Stale) from the context source (clause 6.3.17).
    let mut warnings = Vec::new();
    if let Some(Warning { code: 110, text, .. }) = sent.warning {
        warnings.push(NgsiLdWarning::ResponseIsStale(text));
    }
    // Clause 6.3.17 applies along the whole chain: what the source could not
    // reach behind it is as missing from this response as from its own.
    warnings.extend(sent.ngsild_warnings);
    // Clause 6.3.10: a 206 says where the answer was cut short. One that does
    // not say it cannot be combined with the rest of the result, so the
    // client is told the source's part may be incomplete.
    if sent.status == reqwest::StatusCode::PARTIAL_CONTENT && sent.partial_range.is_none() {
        warnings.push(NgsiLdWarning::MiscellaneousWarning(format!(
            "Context source {registration_id} answered 206 without a readable Content-Range; its part of the history may be incomplete"
        )));
    }

    match serde_json::from_slice::<Value>(&sent.body) {
        Ok(body) => ForwardGetOutcome::Success(ForwardedResponse {
            body,
            warnings,
            entity_map: sent.entity_map,
            continuation: SourceContinuation::of(sent.next_page.as_ref(), answered, registration.access.endpoint.url()),
            partial_range: sent.partial_range,
        }),
        Err(error) => {
            // The typed parse error names the line and column; the peer's body
            // itself is not echoed into the log.
            warn!(registration = %registration_id, %error, "context source answered with a body that is not JSON");
            ForwardGetOutcome::InvalidPayload(error)
        }
    }
}
