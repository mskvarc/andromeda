//! Context-source federation policy, forwarding, and response combination.

#![deny(missing_docs)]

/// Cached forwarded GET responses (clause 6.3.17).
pub mod cache;
/// Federation operation configuration.
pub mod config;
/// Per-registration cooldown after failures (clause 5.2.34).
pub mod cooldown;
/// Context-source discovery request execution.
pub mod discovery;
/// Discovery response merging.
pub mod discovery_result;
/// EntityMap creation forwarding (clauses 5.14.4 and 5.14.5).
pub mod entity_map;
/// Current-entity query and retrieve execution.
pub mod entity_read;
/// Current-entity response merging.
pub mod entity_result;
/// Federation policy failures.
pub mod error;
/// The representation a forwarded request carries.
pub mod forwarded_payload;
/// Forwarded URL construction.
pub mod forwarding_url;
/// NGSI-LD operation capability matching.
pub mod operation;
/// Typed forwarding outcomes.
pub mod outcome;
/// Current-entity provision forwarding.
pub mod provision;
/// Registration-information selection.
pub mod registration_selection;
/// Typed forwarding inputs.
pub mod request;
/// Subscription forwarding to registered context sources (clause 5.8).
pub mod subscription;
/// Temporal query and retrieve execution.
pub mod temporal;
/// Temporal response merging.
pub mod temporal_result;
/// Shared forwarding HTTP transport.
pub mod transport;

use crate::{
    cache::ForwardedGetCache,
    config::{FederationConfig, SplitEntitiesDefault},
    cooldown::CooldownRegistry,
    error::FederationError,
    outcome::TransportFailure,
};
use andromeda_http_headers::{received_protocol::ReceivedProtocol, via::Via, warning::Warning};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::store::{ForwardingOutcome, RegistrationStore};
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tracing::warn;

/// Whether an incoming `Via` chain already contains the local context-source alias.
fn loop_detected(incoming_via: Option<&Via>, source_alias: &ContextSourcePseudonym) -> bool {
    incoming_via.is_some_and(|via| via.contains_pseudonym(source_alias))
}

/// Whether a registration's own alias is already in the incoming `Via` chain,
/// so contacting it would close a loop (clause 4.3.6.4).
fn registration_loops(registration: &ContextSourceRegistration, incoming_via: Option<&Via>) -> bool {
    match (&registration.access.context_source_alias, incoming_via) {
        (Some(alias), Some(via)) => via.contains_pseudonym(alias),
        (None, _) | (_, None) => false,
    }
}

/// What a forwarded request answered with, before the caller reads the body.
pub(crate) struct SentResponse {
    /// The HTTP status.
    pub(crate) status: reqwest::StatusCode,
    /// The IANA `Warning` header the source sent, if any.
    pub(crate) warning: Option<Warning>,
    /// The raw body.
    pub(crate) body: Vec<u8>,
}

/// Service for forwarding NGSI-LD operations to registered context sources.
pub struct FederationService {
    http_client: reqwest::Client,
    csr_repo: Arc<dyn RegistrationStore>,
    engine: JsonLdEngine,
    /// Context Source alias used as the `Via` pseudonym (clause 6.3.18).
    source_alias: ContextSourcePseudonym,
    /// Protocol token appended with the local alias to every forwarded request.
    received_protocol: ReceivedProtocol,
    /// Maximum number of parallel CSR fan-out HTTP requests per operation.
    csr_concurrency: usize,
    /// The deadline a forwarded request gets when its registration sets none.
    default_timeout: Duration,
    /// When each registration last failed, for clause 5.2.34 cooldowns.
    cooldowns: CooldownRegistry,
    /// Forwarded GET responses reused within a registration's `cacheDuration`.
    cache: ForwardedGetCache,
    /// What an unset `splitEntities` means in this deployment.
    split_entities_default: SplitEntitiesDefault,
}

impl FederationService {
    /// Create a forwarding service with its validated `Via` identity.
    pub fn new(
        csr_repo: Arc<dyn RegistrationStore>,
        engine: JsonLdEngine,
        source_alias: ContextSourcePseudonym,
        config: &FederationConfig,
    ) -> Result<FederationService, FederationError> {
        let http_client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(config.connect_timeout())
            .build()
            .map_err(|e| FederationError::internal(format!("Failed to create HTTP client for context source forwarding: {}", e)))?;
        let received_protocol =
            ReceivedProtocol::try_from("1.1").map_err(|error| FederationError::internal(format!("invalid forwarding protocol: {error}")))?;
        Ok(FederationService {
            http_client,
            csr_repo,
            engine,
            source_alias,
            received_protocol,
            csr_concurrency: config.forward_concurrency.max(1),
            default_timeout: config.request_timeout(),
            cooldowns: CooldownRegistry::default(),
            cache: ForwardedGetCache::new(config.cache_max_entries),
            split_entities_default: config.split_entities_default,
        })
    }

    /// Resolve a request's or subscription's `splitEntities` against the
    /// deployment default (clauses 4.3.6.7 and 5.2.12).
    pub const fn split_entities(&self, explicit: Option<bool>) -> bool {
        self.split_entities_default.resolve(explicit)
    }

    /// The JSON-LD engine forwarding expands and compacts with.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// The registration store forwarding selects context sources from.
    pub fn registration_store(&self) -> &dyn RegistrationStore {
        self.csr_repo.as_ref()
    }

    /// Check whether forwarding would create a Via loop.
    pub fn is_loop_detected(&self, incoming_via: Option<&Via>) -> bool {
        loop_detected(incoming_via, &self.source_alias)
    }

    /// The deadline a forwarded request to `registration` gets: its
    /// `management.timeout` (clause 5.2.34) or the configured default.
    pub(crate) fn timeout_for(&self, registration: &ContextSourceRegistration) -> Duration {
        registration
            .access
            .management
            .as_ref()
            .and_then(|management| management.timeout)
            .map_or(self.default_timeout, |timeout| timeout.as_duration())
    }

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
                let warning = headers::HeaderMapExt::typed_get::<Warning>(response.headers());
                match response.bytes().await {
                    Ok(body) => Ok(SentResponse {
                        status,
                        warning,
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
        self.record_health(&registration.id, health);
        outcome
    }

    /// Record one attempt on the registration's health members without
    /// holding up the forwarding that produced it.
    fn record_health(&self, registration: &Uri, outcome: ForwardingOutcome) {
        match outcome {
            ForwardingOutcome::Succeeded => self.cooldowns.record_success(registration),
            ForwardingOutcome::Failed => self.cooldowns.record_failure(registration),
        }
        let repo = Arc::clone(&self.csr_repo);
        let registration = registration.clone(); // clone: the recording task outlives the borrowed registration
        tokio::spawn(async move {
            if let Err(error) = repo.record_forwarding_outcome(&registration, outcome, Utc::now()).await {
                warn!(%registration, %error, "failed to record the forwarding outcome on the registration");
            }
        });
    }
}

/// Sort a `reqwest` failure into the two clause 6.3.17 transport outcomes.
fn classify_transport_failure(error: reqwest::Error, timeout: Duration) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure::Timeout {
            detail: format!("no response within {}ms", timeout.as_millis()),
        }
    } else {
        TransportFailure::Unreachable { detail: error.to_string() }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-registration",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["confique", "reqwest", "tokio"],
}
