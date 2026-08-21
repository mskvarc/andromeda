//! Federated type and attribute discovery queries (clauses 5.7.5 to 5.7.10, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    entity_read::own_loop_warning,
    forwarding_url::build_discovery_url,
    outcome::{ForwardGetOutcome, ForwardedDiscoveryResult},
    request::ForwardDiscoveryRequest,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use futures::{StreamExt, stream};
use tracing::warn;

impl FederationService {
    /// Forward a type or attribute discovery query to every active registration
    /// that advertises the operation.
    pub async fn forward_discovery_query(&self, request: ForwardDiscoveryRequest<'_>) -> Vec<ForwardedDiscoveryResult> {
        let ForwardDiscoveryRequest {
            path,
            query_string,
            context,
            operation,
            triggering,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return vec![ForwardedDiscoveryResult {
                body: None,
                warnings: vec![own_loop_warning()],
            }];
        }

        // Keyset-paginate through all active CSRs; discovery forwarding must
        // consider every registration, but the repository caps each round-trip
        // so a runaway CSR table cannot blow up memory.
        const PAGE: u32 = 500;
        let page_capacity = match usize::try_from(PAGE) {
            Ok(value) => value,
            Err(error) => {
                warn!(%error, "discovery forwarding page size does not fit this platform");
                return Vec::new();
            }
        };
        let mut csrs = Vec::new();
        let mut cursor: Option<Uri> = None;
        loop {
            let page = match self.csr_repo.find_all_active(cursor.as_ref(), PAGE).await {
                Ok(page) => page,
                Err(e) => {
                    warn!("Failed to load active CSRs for discovery forwarding: {}", e);
                    return Vec::new();
                }
            };
            if page.is_empty() {
                break;
            }
            let page_len = page.len();
            cursor = page.last().map(|registration| registration.id.clone()); // clone: the cursor must outlive the page moved below.
            csrs.extend(page.into_iter().filter(|registration| operation.matches(registration)));
            if page_len < page_capacity {
                break;
            }
        }

        if csrs.is_empty() {
            return Vec::new();
        }

        stream::iter(csrs)
            .map(|csr| async move {
                let url = build_discovery_url(&csr.access.endpoint, path, query_string);
                // Clause 6.3.17 allows caching for `/entities` only.
                let outcome = self.forward_get(&csr, url, context, triggering, Cacheability::Uncacheable, None).await;
                match outcome {
                    ForwardGetOutcome::Success(body, cs_warnings) => Some(ForwardedDiscoveryResult {
                        body: Some(body),
                        warnings: cs_warnings,
                    }),
                    // 404 is not abnormal per spec clause 6.3.17
                    ForwardGetOutcome::NotFound => None,
                    other @ (ForwardGetOutcome::InvalidPayload(_)
                    | ForwardGetOutcome::ErrorStatus(_)
                    | ForwardGetOutcome::Transport(_)
                    | ForwardGetOutcome::LoopDetected) => {
                        let warning = other.warning(&csr.id)?;
                        warn!("Discovery forwarding to context source {} failed: {}", csr.id, warning);
                        Some(ForwardedDiscoveryResult {
                            body: None,
                            warnings: vec![warning],
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await
    }
}
