//! Forwarding type and attribute discovery queries.

use crate::service::csource_forwarding::{
    CsourceForwardingService,
    forwarding_url::build_discovery_url,
    outcome::{ForwardGetOutcome, ForwardedDiscoveryResult},
};
use andromeda_ngsi_ld_http_headers::{ngsild_warning::NgsiLdWarning, via::Via};
use tracing::{debug, warn};

impl CsourceForwardingService {
    /// Forward a type or attribute discovery query to active registrations.
    pub async fn forward_discovery_query(
        &self,
        path: &str,
        query_string: &str,
        context_url: Option<&str>,
        incoming_via: Option<&Via>,
    ) -> Vec<ForwardedDiscoveryResult> {
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping discovery forwarding");
            return Vec::new();
        }

        // Keyset-paginate through all active CSRs; discovery forwarding must
        // consider every registration, but the repository caps each round-trip
        // so a runaway CSR table cannot blow up memory.
        const PAGE: u32 = 500;
        let mut csrs = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = match self.csr_repo.find_all_active(cursor.as_deref(), PAGE).await {
                Ok(page) => page,
                Err(e) => {
                    warn!("Failed to load active CSRs for discovery forwarding: {}", e);
                    return Vec::new();
                }
            };
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|c| c.id.to_string());
            csrs.extend(page);
            if csrs.len() % usize::try_from(PAGE).unwrap_or(usize::MAX) != 0 {
                break;
            }
        }

        if csrs.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        for csr in csrs {
            let filtered = match (&csr.access.context_source_alias, incoming_via) {
                (Some(alias), Some(via)) => via.contains_pseudonym(alias),
                _ => false,
            };
            if filtered {
                continue;
            }
            let url = build_discovery_url(&csr.access.endpoint, path, query_string);

            match self.forward_get(url, context_url, csr.tenant.as_ref(), incoming_via).await {
                ForwardGetOutcome::Success(body, cs_warnings) => {
                    results.push(ForwardedDiscoveryResult {
                        body: Some(body),
                        warnings: cs_warnings,
                    });
                }
                ForwardGetOutcome::NotFound => {
                    // 404 is not abnormal per spec clause 6.3.17
                }
                ForwardGetOutcome::InvalidPayload(msg) => {
                    warn!("Context source {} returned invalid discovery payload: {}", csr.id, msg);
                    results.push(ForwardedDiscoveryResult {
                        body: None,
                        warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                            "Context source {} returned invalid payload: {}",
                            csr.id, msg
                        ))],
                    });
                }
                ForwardGetOutcome::ErrorStatus(status) => {
                    warn!("Context source {} returned HTTP {} for discovery", csr.id, status);
                    results.push(ForwardedDiscoveryResult {
                        body: None,
                        warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                            "Context source {} returned HTTP {}",
                            csr.id, status
                        ))],
                    });
                }
                ForwardGetOutcome::NetworkError(msg) => {
                    warn!("Failed to forward discovery to context source {}: {}", csr.id, msg);
                    results.push(ForwardedDiscoveryResult {
                        body: None,
                        warnings: vec![NgsiLdWarning::MiscellaneousWarning(format!(
                            "Context source {} did not respond: {}",
                            csr.id, msg
                        ))],
                    });
                }
            }
        }

        results
    }
}
