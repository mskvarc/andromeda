//! Forwarding temporal entity query and retrieve operations.

use crate::{
    error::AndromedaError,
    service::csource_forwarding::{
        CsourceForwardingService,
        forwarding_url::{build_temporal_query_url, build_temporal_retrieve_url},
        operation::ForwardingOperation,
        outcome::{ForwardGetOutcome, ForwardedTemporalEntities},
        request::{CsrModeFilter, ForwardTemporalQueryRequest},
    },
};
use andromeda_ngsi_ld_http_headers::{ngsild_warning::NgsiLdWarning, via::Via};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use futures::{StreamExt, stream};
use serde_json::Value;
use tracing::{debug, warn};

impl CsourceForwardingService {
    /// Forward a temporal entity query to matching registrations.
    pub async fn forward_temporal_query(
        &self,
        req: &ForwardTemporalQueryRequest<'_>,
        mode_filter: CsrModeFilter,
    ) -> Result<Vec<ForwardedTemporalEntities>, AndromedaError> {
        let entity_type_compact = req.entity_type_compact;
        let entity_ids = req.entity_ids;
        let query_string = req.query_string;
        let context_url = req.context_url;
        let context = req.context;
        let operation = req.operation;
        let incoming_via = req.incoming_via;
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping temporal query forwarding");
            return Ok(Vec::new());
        }

        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_type = if let Some(t) = entity_type_compact {
            Some(self.engine.expand_type(t, &prepared_context).await?.to_string())
        } else {
            None
        };

        let first_id = entity_ids.and_then(|ids| ids.first().map(Uri::as_str));
        let csrs = self.csr_repo.find_matching(expanded_type.as_deref(), first_id).await?;
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

        let results: Vec<ForwardedTemporalEntities> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = build_temporal_query_url(&csr.access.endpoint, query_string);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(url, context_url, csr.tenant.as_ref(), incoming_via).await {
                    ForwardGetOutcome::Success(Value::Array(arr), cs_warnings) => Some(ForwardedTemporalEntities {
                        mode,
                        entities: arr,
                        warnings: cs_warnings,
                    }),
                    ForwardGetOutcome::Success(obj @ Value::Object(_), cs_warnings) => Some(ForwardedTemporalEntities {
                        mode,
                        entities: vec![obj],
                        warnings: cs_warnings,
                    }),
                    ForwardGetOutcome::Success(_other, _) => {
                        warn!("Context source {} returned unexpected temporal response type", csr.id);
                        Some(ForwardedTemporalEntities {
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
                        warn!("Context source {} returned invalid temporal payload: {}", csr.id, msg);
                        Some(ForwardedTemporalEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned invalid payload: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                    ForwardGetOutcome::ErrorStatus(status) => {
                        warn!("Context source {} returned HTTP {} for temporal query", csr.id, status);
                        Some(ForwardedTemporalEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                                "Context source {} returned HTTP {}",
                                csr.id, status
                            ))],
                        })
                    }
                    ForwardGetOutcome::NetworkError(msg) => {
                        warn!("Failed to forward temporal query to context source {}: {}", csr.id, msg);
                        Some(ForwardedTemporalEntities {
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

    /// Forward a temporal entity retrieval to matching context sources.
    ///
    /// Same pattern as `forward_retrieve()` but targets `/ngsi-ld/v1/temporal/entities/{id}`.
    pub async fn forward_temporal_retrieve(
        &self,
        entity_id: &str,
        query_string: &str,
        context_url: Option<&str>,
        operation: ForwardingOperation,
        incoming_via: Option<&Via>,
        mode_filter: CsrModeFilter,
    ) -> Result<Vec<ForwardedTemporalEntities>, AndromedaError> {
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping temporal retrieve forwarding");
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

        let results: Vec<ForwardedTemporalEntities> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = build_temporal_retrieve_url(&csr.access.endpoint, entity_id, query_string);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(url, context_url, csr.tenant.as_ref(), incoming_via).await {
                    ForwardGetOutcome::Success(obj @ Value::Object(_), cs_warnings) => Some(ForwardedTemporalEntities {
                        mode,
                        entities: vec![obj],
                        warnings: cs_warnings,
                    }),
                    ForwardGetOutcome::Success(_other, _) => {
                        warn!("Context source {} returned non-object for temporal retrieve", csr.id);
                        Some(ForwardedTemporalEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned non-object for temporal entity retrieve",
                                csr.id
                            ))],
                        })
                    }
                    ForwardGetOutcome::NotFound => None,
                    ForwardGetOutcome::InvalidPayload(msg) => {
                        warn!("Context source {} returned invalid temporal payload: {}", csr.id, msg);
                        Some(ForwardedTemporalEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::RevalidationFailed(format!(
                                "Context source {} returned invalid payload: {}",
                                csr.id, msg
                            ))],
                        })
                    }
                    ForwardGetOutcome::ErrorStatus(status) => {
                        warn!("Context source {} returned HTTP {} for temporal retrieve", csr.id, status);
                        Some(ForwardedTemporalEntities {
                            mode,
                            entities: Vec::new(),
                            warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                                "Context source {} returned HTTP {}",
                                csr.id, status
                            ))],
                        })
                    }
                    ForwardGetOutcome::NetworkError(msg) => {
                        warn!("Failed to forward temporal retrieve to context source {}: {}", csr.id, msg);
                        Some(ForwardedTemporalEntities {
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
