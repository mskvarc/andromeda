//! Context-source federation policy, forwarding, and response combination.

#![deny(missing_docs)]

/// Context-source discovery request execution.
pub mod discovery;
/// Discovery response merging.
pub mod discovery_result;
/// Current-entity query and retrieve execution.
pub mod entity_read;
/// Current-entity response merging.
pub mod entity_result;
/// Federation policy failures.
pub mod error;
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
/// Temporal query and retrieve execution.
pub mod temporal;
/// Temporal response merging.
pub mod temporal_result;
/// Shared forwarding HTTP transport.
pub mod transport;

use crate::error::FederationError;
use andromeda_http_headers::{received_protocol::ReceivedProtocol, via::Via};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_registration::store::RegistrationStore;
use std::{sync::Arc, time::Duration};

/// Whether an incoming `Via` chain already contains the local context-source alias.
fn loop_detected(incoming_via: Option<&Via>, source_alias: &ContextSourcePseudonym) -> bool {
    incoming_via.is_some_and(|via| via.contains_pseudonym(source_alias))
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
}

impl FederationService {
    /// Create a forwarding service with its validated `Via` identity.
    pub fn new(
        csr_repo: Arc<dyn RegistrationStore>,
        engine: JsonLdEngine,
        source_alias: ContextSourcePseudonym,
        csr_concurrency: usize,
    ) -> Result<FederationService, FederationError> {
        let http_client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
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
            csr_concurrency: csr_concurrency.max(1),
        })
    }

    /// Check whether forwarding would create a Via loop.
    pub fn is_loop_detected(&self, incoming_via: Option<&Via>) -> bool {
        loop_detected(incoming_via, &self.source_alias)
    }

    /// Send a request and return the status and raw bytes.
    pub(crate) async fn send_request(&self, request: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, Option<String>, Vec<u8>), FederationError> {
        let response = request.send().await?;

        let status = response.status();
        let warning = response.headers().get("Warning").and_then(|v| v.to_str().ok()).map(String::from);

        let bytes = response.bytes().await?;

        Ok((status, warning, bytes.to_vec()))
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod boundary {
    use andromeda_layer_guard::manifest::{assert_layer, baseline_forbidden_except};

    /// Federation may own HTTP client execution but no server or storage dialect.
    #[test]
    fn this_crate_declares_only_federation_dependencies() {
        /// In-workspace crates this policy may name in dependency tables.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-http-headers",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-registration",
        ];
        let forbidden = baseline_forbidden_except(&["reqwest"]);
        assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
