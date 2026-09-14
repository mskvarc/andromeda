//! Context-source federation policy, forwarding, and response combination.

#![deny(missing_docs)]

/// The clause 4.5.5 merge of attribute instances from several sources.
pub mod attribute_instance_merge;
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
/// The parameter vocabulary of a forwarded read request.
pub mod forwarded_parameter;
/// The representation a forwarded request carries.
pub mod forwarded_payload;
/// Per-registration narrowing of a client's query parameters.
pub mod forwarded_query;
/// Forwarded URL construction.
pub mod forwarding_url;
/// NGSI-LD operation capability matching.
pub mod operation;
/// Whether a read's answer may be ordered (clauses 4.23.1, 5.7.2.4, 5.7.4.4).
pub mod ordering_admission;
/// Typed forwarding outcomes.
pub mod outcome;
/// Single-entity provision forwarding.
pub mod provision;
/// Batch provision forwarding.
pub mod provision_batch;
/// Purge forwarding.
pub mod provision_purge;
/// Registration selection and fan-out shared by forwarded reads.
pub mod read_fan_out;
/// Registration-information selection.
pub mod registration_selection;
/// Typed forwarding inputs.
pub mod request;
/// Subscription forwarding to registered context sources (clause 5.8).
pub mod subscription;
/// Temporal query and retrieve execution.
pub mod temporal;
/// Post-aggregation `lastN` truncation of temporal representations.
pub mod temporal_last_n;
/// Temporal response merging.
pub mod temporal_result;
/// The clause 4.5.5.2 differentiation of conflicting transient entities.
mod transient_entity_conflict;
/// Shared forwarding HTTP transport.
pub mod transport;

use crate::{
    cache::ForwardedGetCache,
    config::{FederationConfig, SplitEntities, SplitEntitiesDefault},
    cooldown::CooldownRegistry,
    error::FederationError,
    outcome::TransportFailure,
};
use andromeda_http_headers::{
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_warning::{NgsiLdWarning, NgsiLdWarningHeader},
    preference_applied::PreferenceApplied,
    received_protocol::ReceivedProtocol,
    via::Via,
    warning::Warning,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    positive_milliseconds::PositiveMilliseconds,
    spec_version::SpecVersion,
    uri::Uri,
};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::store::{RegistrationStore, forwarding_outcome::ForwardingOutcome};
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tracing::warn;

/// Whether an incoming `Via` chain already contains the local context-source alias.
fn loop_detected(incoming_via: Option<&Via>, source_alias: &ContextSourcePseudonym) -> bool {
    incoming_via.is_some_and(|via| via.contains_pseudonym(source_alias))
}

/// Whether contacting a registration would close a loop (clauses 4.3.6.4 and
/// 6.3.17): its alias is already in the incoming `Via` chain, or it is this
/// broker's own alias, which is the "registered to redirect back on to the
/// Context Broker" case clause 6.3.17 names.
fn registration_loops(registration: &ContextSourceRegistration, incoming_via: Option<&Via>, local_alias: &ContextSourcePseudonym) -> bool {
    let Some(alias) = &registration.access.context_source_alias else {
        return false;
    };
    alias == local_alias || incoming_via.is_some_and(|via| via.contains_pseudonym(alias))
}

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
    pub(crate) entity_map: Option<Uri>,
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
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Internal`] when the forwarding HTTP client
    /// cannot be built from `config`, or when the `Received-Protocol` token
    /// the forwarded `Via` header carries cannot be constructed.
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
            .map_err(|e| FederationError::internal(format!("Failed to create HTTP client for context source forwarding: {e}")))?;
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
    #[must_use]
    pub const fn split_entities(&self, explicit: Option<bool>) -> SplitEntities {
        self.split_entities_default.resolve(explicit)
    }

    /// The JSON-LD engine forwarding expands and compacts with.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// The registration store forwarding selects context sources from.
    #[must_use]
    pub fn registration_store(&self) -> &dyn RegistrationStore {
        self.csr_repo.as_ref()
    }

    /// Check whether forwarding would create a Via loop.
    #[must_use]
    pub fn is_loop_detected(&self, incoming_via: Option<&Via>) -> bool {
        loop_detected(incoming_via, &self.source_alias)
    }

    /// Whether contacting `registration` would close a loop, through the
    /// incoming `Via` chain or by pointing back at this broker's own alias.
    pub(crate) fn registration_loops(&self, registration: &ContextSourceRegistration, incoming_via: Option<&Via>) -> bool {
        registration_loops(registration, incoming_via, &self.source_alias)
    }

    /// The deadline a forwarded request to `registration` gets: its
    /// `management.timeout` (clause 5.2.34) or the configured default.
    pub(crate) fn timeout_for(&self, registration: &ContextSourceRegistration) -> Duration {
        registration
            .access
            .management
            .as_ref()
            .and_then(|management| management.timeout)
            .map_or(self.default_timeout, PositiveMilliseconds::as_duration)
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
                let ngsild_warnings = headers::HeaderMapExt::typed_get::<NgsiLdWarningHeader>(response.headers())
                    .map(NgsiLdWarningHeader::into_warnings)
                    .unwrap_or_default();
                let returned_version = headers::HeaderMapExt::typed_get::<PreferenceApplied>(response.headers()).and_then(|applied| applied.applied_version());
                let entity_map = headers::HeaderMapExt::typed_get::<NgsiLdEntityMap>(response.headers()).map(NgsiLdEntityMap::into_inner);
                match response.bytes().await {
                    Ok(body) => Ok(SentResponse {
                        status,
                        warning,
                        ngsild_warnings,
                        returned_version,
                        entity_map,
                        body: body.to_vec(),
                    }),
                    Err(error) => Err(classify_transport_failure(&error, timeout)),
                }
            }
            Err(error) => Err(classify_transport_failure(&error, timeout)),
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
    async fn record_health(&self, registration: &Uri, outcome: ForwardingOutcome) {
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
fn classify_transport_failure(error: &reqwest::Error, timeout: Duration) -> TransportFailure {
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
        "andromeda-context-space",
        "andromeda-entity-map",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-registration",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["reqwest", "tokio"],
}
