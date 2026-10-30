//! What a registered Context Source says about its own configuration
//! (clauses 5.15.1 and 5.2.40).
//!
//! A Context Source Registration (clause 5.2.9) carries nothing about what the
//! source supports beyond the operations it serves. The one place the
//! specification lets a source describe its own configuration is the
//! `contextSourceExtras` member of its identity, "instance specific
//! information relevant to the configuration of the Context Source itself"
//! (Table 5.2.40-1), answered at `GET /info/sourceIdentity`. A broker that
//! must not send a source a request outside the specification unless the
//! source declared it understands it reads that member here.
//!
//! The identity is asked of the source, not of the data it holds, so the
//! request carries the registration's tenant and `contextSourceInfo` headers
//! but no local scope. A source that cannot answer, or answers clause
//! 5.15.1.4's `NotImplemented`, has described nothing. Neither outcome is a
//! forwarding failure: the clause 5.2.34 health of the registration is left
//! as the forwarded operations record it.

use crate::{
    FederationService,
    error::FederationError,
    forwarded_payload::ForwardedPayload,
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    participating_read::ParticipatingRead,
    registration_selection::{RegistrationSelection, RegistrationServing},
    request::{CsrModeFilter, EntityMapInUse, TriggeringRequest},
    source_reach::SourceReach,
    transport::ForwardedRequest,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;
use dashmap::DashMap;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tracing::debug;
use url::Url;

/// How long a source's answer is reused before it is asked again.
///
/// A source's configuration changes when it is redeployed, not between two
/// requests, and asking it once per Linked Entity would double the round
/// trips the probe exists to save.
const IDENTITY_LIFETIME: Duration = Duration::from_secs(300);

/// The identity path below the API base (clause 6.33).
const IDENTITY_PATH: [&str; 2] = ["info", "sourceIdentity"];

/// What one registered Context Source said about its own configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceConfiguration {
    /// It answered with its identity, carrying these `contextSourceExtras`.
    Described(Option<JsonLiteral>),
    /// It could not be asked, or answered without an identity.
    Undescribed,
}

/// What one source a read reaches said about its configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedSource {
    /// The registration naming the source.
    pub registration: Identifier,
    /// What the source said.
    pub configuration: SourceConfiguration,
}

/// The one member of a Context Source identity the probe reads (Table
/// 5.2.40-1); every other member is the source's to state and is ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnsweredIdentity {
    /// The source's instance-specific configuration, raw JSON never expanded.
    context_source_extras: Option<JsonLiteral>,
}

/// One registered source, as the probe answers are kept: a registration
/// pointed at another endpoint is another source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProbedSource {
    /// The registration naming the source.
    registration: Identifier,
    /// The endpoint the registration names.
    endpoint: Url,
}

/// One answer, with the instant it was received.
#[derive(Debug, Clone)]
struct ProbeAnswer {
    /// When the source answered.
    received: Instant,
    /// What it answered.
    configuration: SourceConfiguration,
}

/// The answers the probe reuses within [`IDENTITY_LIFETIME`].
///
/// Holds one entry per registration a temporal Linked Entity was resolved
/// through, which the registration store already bounds.
#[derive(Debug, Default)]
pub(crate) struct SourceIdentityCache {
    /// The answers, by source.
    answers: DashMap<ProbedSource, ProbeAnswer>,
}

impl SourceIdentityCache {
    /// The answer `source` gave within the lifetime, if any.
    fn fresh(&self, source: &ProbedSource, now: Instant) -> Option<SourceConfiguration> {
        self.answers
            .get(source)
            .filter(|answer| now.saturating_duration_since(answer.received) < IDENTITY_LIFETIME)
            .map(|answer| answer.configuration.clone()) // clone: the cache keeps its own answer
    }

    /// Keep what `source` answered at `received`.
    fn keep(&self, source: ProbedSource, received: Instant, configuration: SourceConfiguration) {
        self.answers.insert(source, ProbeAnswer { received, configuration });
    }
}

impl FederationService {
    /// What every Context Source a Retrieve Temporal Evolution of `entity_id`
    /// would be forwarded to says about its own configuration, one answer per
    /// matching registration.
    ///
    /// The registrations are selected exactly as the forwarded retrieval
    /// selects them (clause 5.7.3.4), auxiliary ones excluded (clause
    /// 4.3.6.2), so the answers describe the sources that retrieval reaches.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Registration`] when the matching
    /// registrations cannot be read, and [`FederationError::Conflict`] when
    /// they are mutually exclusive. A source that cannot be asked answers
    /// [`SourceConfiguration::Undescribed`] instead.
    pub async fn temporal_retrieval_configurations(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        context: &ContextSpec,
    ) -> Result<Vec<SourceConfiguration>, FederationError> {
        if !target.permits_federation() {
            return Ok(Vec::new());
        }
        let registrations = self
            .select_registrations(
                context,
                RegistrationSelection {
                    selectors: &[EntitySelectorParams {
                        id: Some(vec![entity_id.clone()]), // clone: the selector owns its identifiers
                        ..EntitySelectorParams::default()
                    }],
                    reach: SourceReach::Every,
                    csf: None,
                    scope_q: None,
                    geo: None,
                    serving: RegistrationServing::Operation(ForwardingOperation::RetrieveTemporalEntity),
                    mode_filter: CsrModeFilter::ExcludeAuxiliary,
                    entity_map: EntityMapInUse::None,
                },
            )
            .await?;
        let mut configurations = Vec::with_capacity(registrations.len());
        for registration in &registrations {
            configurations.push(self.source_configuration(registration).await);
        }
        Ok(configurations)
    }

    /// What every Context Source `read` reaches says about its own
    /// configuration, one answer per participating registration.
    ///
    /// The registrations are those the read's participation is decided by, so
    /// the answers describe the sources a forwarded query or retrieval reaches.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type cannot
    /// be expanded, and [`FederationError::Registration`] when the matching
    /// registrations cannot be read. A source that cannot be asked answers
    /// [`SourceConfiguration::Undescribed`] instead.
    pub async fn query_configurations(
        &self,
        target: &ContextTarget,
        context: &ContextSpec,
        read: &ParticipatingRead<'_>,
    ) -> Result<Vec<ReachedSource>, FederationError> {
        let registrations = self.participating_registrations(target, context, read).await?;
        let mut reached = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let configuration = self.source_configuration(&registration).await;
            reached.push(ReachedSource {
                registration: registration.id,
                configuration,
            });
        }
        Ok(reached)
    }

    /// What the source `registration` names says about its configuration,
    /// asked at most once per [`IDENTITY_LIFETIME`].
    async fn source_configuration(&self, registration: &ContextSourceRegistration) -> SourceConfiguration {
        let source = ProbedSource {
            registration: registration.id.clone(),                // clone: the cache key owns the registration it names
            endpoint: registration.access.endpoint.url().clone(), // clone: the cache key owns the endpoint it names
        };
        let now = Instant::now();
        if let Some(configuration) = self.source_identities.fresh(&source, now) {
            return configuration;
        }
        let configuration = self.ask_identity(registration).await;
        self.source_identities.keep(source, now, configuration.clone()); // clone: the caller and the cache each keep the answer
        configuration
    }

    /// Ask the source `registration` names for its identity.
    async fn ask_identity(&self, registration: &ContextSourceRegistration) -> SourceConfiguration {
        let payload = ForwardedPayload {
            media_type: ResponseMediaType::Json,
            link_context: None,
            body: None,
        };
        let request = forwarding_url(registration.access.endpoint.url(), &IDENTITY_PATH, None).and_then(|url| {
            self.build_unscoped_request(ForwardedRequest {
                method: reqwest::Method::GET,
                url,
                payload: &payload,
                registration,
                triggering: TriggeringRequest::default(),
                linked_map: None,
            })
        });
        let request = match request {
            Ok(request) => request.timeout(self.timeout_for(registration)),
            Err(error) => {
                debug!(registration = %registration.id, %error, "the identity request to the context source cannot be built");
                return SourceConfiguration::Undescribed;
            }
        };
        let response = match self.send_guarded(request).await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                debug!(registration = %registration.id, status = %response.status(), "the context source answered no identity");
                return SourceConfiguration::Undescribed;
            }
            Err(error) => {
                debug!(registration = %registration.id, %error, "the context source could not be asked for its identity");
                return SourceConfiguration::Undescribed;
            }
        };
        match response.json::<AnsweredIdentity>().await {
            Ok(identity) => SourceConfiguration::Described(identity.context_source_extras),
            Err(error) => {
                debug!(registration = %registration.id, %error, "the context source answered an unreadable identity");
                SourceConfiguration::Undescribed
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
