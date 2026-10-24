//! Context-source federation policy, forwarding, and response combination.

#![deny(missing_docs)]

/// The body a context source answered a forwarded write with.
pub mod answer_body;
/// Reading a context source's answer of entities into the model.
mod answer_reading;
/// Reading every source's answer of Temporal Evolutions into the model.
mod answered_temporal;
/// Which attributes of a forwarded write a registration covers.
mod attribute_coverage;
/// Cached forwarded GET responses (clause 6.3.17).
pub mod cache;
/// Federation operation configuration.
pub mod config;
/// Cooldown windows after failed forwarded requests, by endpoint and by registration (Table 5.2.34-1).
pub mod cooldown;
/// The broker's own cooldown after a failed forwarded request (Table 5.2.34-1, clause 6.3.17).
mod cooldown_policy;
/// Context-source discovery request execution.
pub mod discovery;
/// The warnings of a discovery answer that is not merged in full.
mod discovery_answer_failure;
/// Discovery response merging.
pub mod discovery_result;
/// The entities one forwarded query may gather from its sources.
pub mod entity_budget;
/// EntityMap creation forwarding (clauses 5.14.4 and 5.14.5).
pub mod entity_map;
/// Current-entity query and retrieve execution.
pub mod entity_read;
/// Why one entity a source answered is not read into the model.
mod entity_reading;
/// Current-entity response merging.
pub mod entity_result;
/// Dropping source entities a write to this broker would refuse (clause 6.3.17).
/// Federation policy failures.
pub mod error;
/// The concrete reasons a single forwarded write is reported as a conflict.
pub mod federation_conflict_reason;
/// The invariants federation relies on.
pub mod federation_invariant;
/// The concrete reasons a single registered source cannot serve an operation.
pub mod federation_operation_unsupported;
/// A fixed in-memory registration store for the forwarding tests.
#[cfg(test)]
mod fixed_registration_store;
/// The query a forwarded batch entity query sends (clause 6.23).
mod forwarded_batch_query;
/// What a forwarded write carries: the admitted content.
pub mod forwarded_content;
/// Spelling the names a forwarded request carries in its forwarding context.
mod forwarded_names;
/// The parameter vocabulary of a forwarded read request.
pub mod forwarded_parameter;
/// The representation a forwarded request carries.
pub mod forwarded_payload;
/// Per-registration narrowing of a client's query parameters.
pub mod forwarded_query;
/// Spelling the name-bearing parameters of a forwarded query string.
mod forwarded_query_string;
/// Spelling a reduced subscription in a forwarding context.
mod forwarded_subscription;
/// Sending forwarded writes and classifying their answers.
mod forwarded_write;
/// The one context a forwarded request is expressed against.
mod forwarding_context;
/// Forwarded URL construction.
pub mod forwarding_url;
/// Why a single forwarded request failed as a bad gateway.
pub mod gateway_failure;
/// Asking a source for the instances a split temporal query's filters read
/// when its window opens.
pub mod in_effect_probe;
/// The warning of a paged answer the walk could not read to its end.
pub mod incomplete_answer;
/// The most attribute instances one forwarded temporal query may gather.
pub mod instance_budget;
/// How a read's `q` target languages are matched across the sources it reaches.
pub mod language_negotiation;
/// Resolving a locally-held entity's type for federated-write coverage.
pub mod local_entity_type;
/// A mock Context Source answering the forwarding tests.
#[cfg(test)]
mod mock_source;
/// NGSI-LD operation capability matching.
pub mod operation;
/// Whether a read's answer may be ordered (clauses 4.23.1, 5.7.2.4, 5.7.4.4).
pub mod ordering_admission;
/// Typed forwarding outcomes.
pub mod outcome;
/// Which registrations take part in a read.
pub mod participating_read;
/// Single-entity provision forwarding.
pub mod provision;
/// Batch provision forwarding.
pub mod provision_batch;
/// Which registrations a single-entity provision is forwarded to.
pub mod provision_plan;
/// How the type selector of a write narrows the registrations it is forwarded to.
#[cfg(test)]
mod provision_plan_selector_tests;
/// Purge forwarding.
pub mod provision_purge;
/// Why a forwarded entity query produced no answer.
pub mod query_forwarding_error;
/// Choosing how a forwarded current-entity read reaches one registration.
pub mod read_binding;
/// Registration selection and fan-out shared by forwarded reads.
pub mod read_fan_out;
/// Entity versions in the model, for the merge tests.
#[cfg(test)]
mod received_fixture;
/// A context source's answer of Temporal Evolutions, decoded.
mod received_temporal_answer;
/// The forwarded queries one registration receives.
mod registration_narrowing;
/// The context and representation one registration receives.
mod registration_payload;
/// What a context source answered a forwarded request with when it failed it.
pub mod remote_answer;
/// Typed forwarding inputs.
pub mod request;
/// Clause 6.3.17 single-source failure classification.
pub mod single_source_failure;
/// The warnings of a source's answer that is not read in full.
mod source_answer_failure;
/// Where a source's answer to a forwarded read continues.
mod source_continuation;
/// Sending one forwarded request and reading its answer's headers.
mod source_exchange;
/// What a registered Context Source says about its own configuration.
pub mod source_identity;
/// Reads forwarded to one registration and how their answers are read back.
mod source_read;
/// The warnings of a forwarded read that failed or came back partial.
mod source_read_warning;
/// Walking every page of a source's answer to a forwarded query.
pub mod source_walk;
/// Subscription forwarding to registered context sources (clause 5.8).
pub mod subscription;
/// Temporal query and retrieve execution.
pub mod temporal;
/// Reading a source's temporal history to the end of its clause 6.3.10 pages.
mod temporal_history_walk;
/// Temporal response merging.
pub mod temporal_result;
/// Expanding NGSI-LD names against one operation's inbound `@context`.
mod term_resolver;
/// Shared forwarding HTTP transport.
pub mod transport;
/// Reading what a context source answered a forwarded write with.
mod write_answer;

use crate::{
    cache::ForwardedGetCache,
    config::{FederationConfig, SplitEntities, SplitEntitiesDefault},
    cooldown::CooldownRegistry,
    cooldown_policy::CooldownPolicy,
    error::FederationError,
    federation_invariant::FederationInvariant,
    local_entity_type::LocalEntityTypeResolver,
    source_identity::SourceIdentityCache,
};
use andromeda_http_headers::{received_protocol::ReceivedProtocol, via::Via};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, positive_milliseconds::PositiveMilliseconds};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::store::RegistrationStore;
use std::{num::NonZeroU32, sync::Arc, time::Duration};

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

/// Service for forwarding NGSI-LD operations to registered context sources.
pub struct FederationService {
    http_client: reqwest::Client,
    csr_repo: Arc<dyn RegistrationStore>,
    /// Resolves a locally-held entity's type so an attribute-level write whose
    /// request carries no `type` is matched against registrations by the type
    /// the entity actually has, rather than treated as covering every type.
    local_entity_types: Arc<dyn LocalEntityTypeResolver>,
    engine: JsonLdEngine,
    /// Context Source alias used as the `Via` pseudonym (clause 6.3.18).
    source_alias: ContextSourcePseudonym,
    /// Protocol token appended with the local alias to every forwarded request.
    received_protocol: ReceivedProtocol,
    /// Maximum number of parallel CSR fan-out HTTP requests per operation.
    csr_concurrency: usize,
    /// The deadline a forwarded request gets when its registration sets none.
    default_timeout: Duration,
    /// The open cooldown windows that decline forwarded requests (clause 6.3.17).
    cooldowns: CooldownRegistry,
    /// Forwarded GET responses reused within a registration's `cacheDuration`.
    cache: ForwardedGetCache,
    /// What registered sources said about their own configuration.
    source_identities: SourceIdentityCache,
    /// What an unset `splitEntities` means in this deployment.
    split_entities_default: SplitEntitiesDefault,
    /// Most entities one federated query gathers (clause 4.12).
    max_query_entities: NonZeroU32,
    /// Most attribute instances one federated temporal query gathers when it
    /// reads its sources' history to the end (clause 4.12).
    max_query_instances: NonZeroU32,
}

impl FederationService {
    /// Create a forwarding service with its validated `Via` identity.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Internal`] when the forwarding HTTP client
    /// cannot be built from `config`, when the `Received-Protocol` token the
    /// forwarded `Via` header carries cannot be constructed, or when the
    /// cooldown policy of `config` does not validate.
    pub fn new(
        csr_repo: Arc<dyn RegistrationStore>,
        local_entity_types: Arc<dyn LocalEntityTypeResolver>,
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
            .map_err(|error| FederationError::from(FederationInvariant::HttpClient(error)))?;
        let received_protocol = ReceivedProtocol::try_from("1.1").map_err(|error| FederationError::from(FederationInvariant::ForwardingProtocol(error)))?;
        Ok(FederationService {
            http_client,
            csr_repo,
            local_entity_types,
            engine,
            source_alias,
            received_protocol,
            csr_concurrency: config.forward_concurrency.max(1),
            default_timeout: config.request_timeout(),
            cooldowns: CooldownRegistry::new(
                CooldownPolicy::try_from(config).map_err(|error| FederationError::from(FederationInvariant::CooldownPolicy(error)))?,
            ),
            cache: ForwardedGetCache::new(config.cache_max_entries),
            source_identities: SourceIdentityCache::default(),
            split_entities_default: config.split_entities_default,
            max_query_entities: config.max_query_entities,
            max_query_instances: config.max_query_instances,
        })
    }

    /// Most entities one federated Query Entities gathers from the local store
    /// and its context sources together before it is refused with
    /// `TooManyResults` (clause 4.12).
    #[must_use]
    pub const fn max_query_entities(&self) -> NonZeroU32 {
        self.max_query_entities
    }

    /// Most attribute instances one federated temporal query gathers from the
    /// context sources whose history it reads to the end, before it is refused
    /// with `TooManyResults` (clause 4.12).
    #[must_use]
    pub const fn max_query_instances(&self) -> NonZeroU32 {
        self.max_query_instances
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
}

#[cfg(test)]
mod tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-context-space",
        "andromeda-entity-map",
        "andromeda-http-headers",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-registration",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["reqwest", "tokio"],
}
