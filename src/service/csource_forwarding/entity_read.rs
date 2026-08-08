//! Forwarding current-entity query and retrieve operations.

use crate::{
    error::AndromedaError,
    service::csource_forwarding::{
        CsourceForwardingService,
        entity_result::parse_forwarded_entities,
        forwarding_url::{build_forwarding_query_string, build_query_url, build_retrieve_url},
        operation::ForwardingOperation,
        outcome::{ForwardGetOutcome, ForwardedEntities},
        request::{CsrModeFilter, ForwardQueryRequest},
    },
};
use andromeda_ngsi_ld_http_headers::{ngsild_warning::NgsiLdWarning, via::Via};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use andromeda_ngsi_ld_query::filter::eval::evaluate as eval_q_expr;
use futures::{StreamExt, stream};
use serde_json::Value;
use tracing::{debug, warn};

impl CsourceForwardingService {
    /// Forward an entity query to matching context sources.
    ///
    /// `entity_type_compact` is the compact entity type from the query (e.g., "Vehicle").
    /// It will be expanded using the context for CSR matching.
    /// `entity_ids` are optional entity IDs to filter by (for query-by-id forwarding).
    pub async fn forward_query(&self, request: ForwardQueryRequest<'_>) -> Result<Vec<ForwardedEntities>, AndromedaError> {
        let ForwardQueryRequest {
            entity_type_compact,
            entity_ids,
            context_url,
            context,
            operation,
            incoming_via,
            csf_expr,
            mode_filter,
        } = request;

        // Loop detection: if our identity is already in the Via chain, skip forwarding (section 6.3.18)
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping forwarding");
            return Ok(Vec::new());
        }

        // Expand entity type for CSR matching (CSRs store expanded IRIs)
        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_type = if let Some(t) = entity_type_compact {
            Some(self.engine.expand_type(t, &prepared_context).await?.to_string())
        } else {
            None
        };

        // For CSR matching, also pass entity IDs if available
        let first_id = entity_ids.and_then(|ids| ids.first().map(Uri::as_str));
        let csrs = self.csr_repo.find_matching(expanded_type.as_deref(), first_id).await?;
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| operation.matches(csr))
            .filter(|csr| match (&csr.access.context_source_alias, incoming_via) {
                (Some(alias), Some(via)) => !via.contains_pseudonym(alias),
                _ => true,
            })
            .filter(|csr| {
                // Filter CSRs by csf expression against their additional_properties (S6.4.3.2)
                match csf_expr {
                    Some(expr) => match &csr.additional_properties {
                        Some(props) => eval_q_expr(expr, props),
                        None => false,
                    },
                    None => true,
                }
            })
            // Per spec 4.3.6.2: auxiliary CSRs only supplement existing data
            .filter(|csr| match mode_filter {
                CsrModeFilter::All => true,
                CsrModeFilter::ExcludeAuxiliary => csr.access.mode != Some(ForwardingMode::Auxiliary),
                CsrModeFilter::AuxiliaryOnly => csr.access.mode == Some(ForwardingMode::Auxiliary),
            })
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        // Build a minimal forwarding query string — only type and id params
        // Context sources return all attributes; attribute filtering is done locally after merge
        let forwarding_qs = build_forwarding_query_string(entity_type_compact, entity_ids);

        let forwarding_qs_ref = &forwarding_qs;
        let results: Vec<ForwardedEntities> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = build_query_url(&csr.access.endpoint, forwarding_qs_ref);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(url, context_url, csr.tenant.as_ref(), incoming_via).await {
                    ForwardGetOutcome::Success(Value::Array(arr), cs_warnings) => {
                        let (entities, parse_warnings) = parse_forwarded_entities(csr.id.as_ref(), arr);
                        let mut warnings = cs_warnings;
                        warnings.extend(parse_warnings);
                        Some(ForwardedEntities { mode, entities, warnings })
                    }
                    ForwardGetOutcome::Success(obj @ Value::Object(_), cs_warnings) => {
                        let (entities, parse_warnings) = parse_forwarded_entities(csr.id.as_ref(), vec![obj]);
                        let mut warnings = cs_warnings;
                        warnings.extend(parse_warnings);
                        Some(ForwardedEntities { mode, entities, warnings })
                    }
                    ForwardGetOutcome::Success(_other, _) => {
                        warn!("Context source {} returned unexpected response type", csr.id);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned unexpected response type",
                                csr.id
                            ))],
                        })
                    }
                    ForwardGetOutcome::NotFound => None,
                    ForwardGetOutcome::InvalidPayload(msg) => {
                        warn!("Context source {} returned invalid payload: {}", csr.id, msg);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned invalid payload: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                    ForwardGetOutcome::ErrorStatus(status) => {
                        warn!("Context source {} returned HTTP {}", csr.id, status);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                                "Context source {} returned HTTP {}",
                                csr.id, status
                            ))],
                        })
                    }
                    ForwardGetOutcome::NetworkError(msg) => {
                        warn!("Failed to forward query to context source {}: {}", csr.id, msg);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousWarning(format!(
                                "Context source {} did not respond: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        Ok(results)
    }

    /// Forward an entity retrieval to matching context sources.
    ///
    /// Returns entities collected from all matching context sources.
    pub async fn forward_retrieve(
        &self,
        entity_id: &str,
        query_string: &str,
        context_url: Option<&str>,
        operation: ForwardingOperation,
        incoming_via: Option<&Via>,
        mode_filter: CsrModeFilter,
    ) -> Result<Vec<ForwardedEntities>, AndromedaError> {
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping retrieve forwarding");
            return Ok(Vec::new());
        }

        let csrs = self.csr_repo.find_matching(None, Some(entity_id)).await?;
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| operation.matches(csr))
            .filter(|csr| match (&csr.access.context_source_alias, incoming_via) {
                (Some(alias), Some(via)) => !via.contains_pseudonym(alias),
                _ => true,
            })
            // Per spec 4.3.6.2: auxiliary CSRs only supplement existing data
            .filter(|csr| match mode_filter {
                CsrModeFilter::All => true,
                CsrModeFilter::ExcludeAuxiliary => csr.access.mode != Some(ForwardingMode::Auxiliary),
                CsrModeFilter::AuxiliaryOnly => csr.access.mode == Some(ForwardingMode::Auxiliary),
            })
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let results: Vec<ForwardedEntities> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = build_retrieve_url(&csr.access.endpoint, entity_id, query_string);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(url, context_url, csr.tenant.as_ref(), incoming_via).await {
                    ForwardGetOutcome::Success(obj @ Value::Object(_), cs_warnings) => {
                        let (entities, parse_warnings) = parse_forwarded_entities(csr.id.as_ref(), vec![obj]);
                        let mut warnings = cs_warnings;
                        warnings.extend(parse_warnings);
                        Some(ForwardedEntities { mode, entities, warnings })
                    }
                    ForwardGetOutcome::Success(_other, _) => {
                        warn!("Context source {} returned non-object for entity retrieve", csr.id);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned non-object for entity retrieve",
                                csr.id
                            ))],
                        })
                    }
                    ForwardGetOutcome::NotFound => None,
                    ForwardGetOutcome::InvalidPayload(msg) => {
                        warn!("Context source {} returned invalid payload: {}", csr.id, msg);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned invalid payload: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                    ForwardGetOutcome::ErrorStatus(status) => {
                        warn!("Context source {} returned HTTP {}", csr.id, status);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                                "Context source {} returned HTTP {}",
                                csr.id, status
                            ))],
                        })
                    }
                    ForwardGetOutcome::NetworkError(msg) => {
                        warn!("Failed to forward retrieve to context source {}: {}", csr.id, msg);
                        Some(ForwardedEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousWarning(format!(
                                "Context source {} did not respond: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        Ok(results)
    }
}
