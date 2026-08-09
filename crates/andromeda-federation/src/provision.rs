//! Federated current-entity provision operations.

use crate::{
    FederationService,
    error::FederationError,
    forwarding_url::{build_create_url, forwarding_url},
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    registration_selection::{collect_exclusive_attrs, collect_matching_attrs, csr_has_matching_information, filter_fragment_for_registration},
    request::{ForwardBatchProvisionRequest, ForwardProvisionRequest},
};
use andromeda_http_headers::{media_type::APPLICATION_LD_JSON, via::Via};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use futures::{StreamExt, stream};
use iri_rs::IriRefBuf;
use serde_json::Value;
use tracing::{debug, warn};
use url::Url;

impl FederationService {
    /// Forward a single NGSI-LD provision operation.
    pub async fn forward_provision(&self, request: ForwardProvisionRequest<'_>) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        if self.is_loop_detected(request.incoming_via) {
            debug!("Loop detected in Via header chain, skipping provision forwarding");
            return Ok(Vec::new());
        }

        let prepared_context = self.engine.prepare_term_context(request.context);
        let expanded_type = if let Some(t) = request.entity_type_compact {
            Some(self.engine.expand_type(t.as_str(), &prepared_context).await?)
        } else {
            None
        };

        let csrs = self.csr_repo.find_matching(expanded_type.as_ref(), Some(request.entity_id)).await?;

        // Separate CSRs into those that match the operation and exclusive ones that don't.
        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let mut matching_csrs = Vec::new();
        let mut unforwardable_exclusive = false;
        for csr in csrs {
            // Skip auxiliary CSRs for provision operations (auxiliary is retrieval-only)
            if csr.access.mode == Some(ForwardingMode::Auxiliary) {
                continue;
            }
            let filtered = match (&csr.access.context_source_alias, request.incoming_via) {
                (Some(alias), Some(via)) => via.contains_pseudonym(alias),
                _ => false,
            };
            if filtered {
                continue;
            }
            if !csr_has_matching_information(&csr, expanded_type.as_ref(), Some(request.entity_id), request.attribute_name) {
                continue;
            }
            if request.operation.matches(&csr) {
                matching_csrs.push(csr);
            } else if csr.access.mode == Some(ForwardingMode::Exclusive) {
                // Per spec 4.3.6.3: exclusive CSR exists but doesn't support this operation
                unforwardable_exclusive = true;
            }
        }

        if unforwardable_exclusive {
            return Err(FederationError::conflict(
                "An applicable exclusive context source does not advertise the requested operation",
            ));
        }

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let request_ref = &request;
        let expanded_type_ref = expanded_type.as_ref();
        let results: Vec<ForwardedWriteResult> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = if request_ref.method == reqwest::Method::POST && request_ref.path_suffix.is_none() {
                    build_create_url(&csr.access.endpoint)
                } else {
                    let mut path = vec!["ngsi-ld", "v1", "entities", request_ref.entity_id.as_str()];
                    path.extend(
                        request_ref
                            .path_suffix
                            .into_iter()
                            .flat_map(|suffix| suffix.split('/'))
                            .filter(|segment| !segment.is_empty()),
                    );
                    forwarding_url(&csr.access.endpoint, &path, request_ref.query_string.unwrap_or(""))
                };

                let exclusive_attrs = if csr.access.mode == Some(ForwardingMode::Exclusive) {
                    collect_matching_attrs(
                        &csr,
                        expanded_type_ref,
                        Some(request_ref.entity_id),
                        request_ref.attribute_name,
                        request_ref.body,
                    )
                } else {
                    Vec::new()
                };

                let filtered_body = if request_ref.attribute_name.is_none() {
                    filter_fragment_for_registration(request_ref.body, &csr, expanded_type_ref, Some(request_ref.entity_id))
                } else {
                    request_ref.body.map(<[u8]>::to_vec)
                };
                if request_ref.body.is_some() && filtered_body.is_none() {
                    return None;
                }

                match self
                    .forward_write(
                        url,
                        request_ref.method.clone(),
                        filtered_body.as_deref(),
                        request_ref.context_url,
                        csr.tenant.as_ref(),
                        request_ref.incoming_via,
                    )
                    .await
                {
                    Ok((status, response_body)) => {
                        let mode = csr.access.mode.unwrap_or_default();
                        let outcome = if status < 400 {
                            ForwardedWriteOutcome::Completed { status, body: response_body }
                        } else {
                            ForwardedWriteOutcome::RemoteHttpFailure { status, body: response_body }
                        };
                        Some(ForwardedWriteResult {
                            mode,
                            outcome,
                            exclusive_attrs,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to forward {} to context source {}: {}", request_ref.method, csr.id, e);
                        let mode = csr.access.mode.unwrap_or_default();
                        Some(ForwardedWriteResult {
                            mode,
                            outcome: ForwardedWriteOutcome::TransportFailure { detail: e.to_string() },
                            exclusive_attrs,
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        if results
            .iter()
            .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) && result.is_failure())
        {
            return Err(FederationError::context_source_gateway(
                "An exclusive or redirect context source failed to complete the operation",
            ));
        }
        Ok(results)
    }

    /// Forward a batch provision operation to matching context sources.
    ///
    /// Per spec 5.6.7-5.6.10, 5.6.20: batch operations iterate over CSRs,
    /// filter the input array per CSR, and forward matching entities.
    /// `batch_path` is the operation endpoint (e.g., "/entityOperations/create").
    pub async fn forward_batch_provision(&self, request: ForwardBatchProvisionRequest<'_>) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        if self.is_loop_detected(request.incoming_via) {
            debug!("Loop detected in Via header chain, skipping batch provision forwarding");
            return Ok(Vec::new());
        }

        let prepared_context = self.engine.prepare_term_context(request.context);
        let expanded_type = if let Some(t) = request.entity_type_compact {
            Some(self.engine.expand_type(t.as_str(), &prepared_context).await?)
        } else {
            None
        };

        let csrs = self.csr_repo.find_matching(expanded_type.as_ref(), None).await?;
        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let matching_csrs: Vec<_> = csrs
            .into_iter()
            .filter(|csr| csr.access.mode != Some(ForwardingMode::Auxiliary))
            .filter(|csr| request.operation.matches(csr))
            .filter(|csr| match (&csr.access.context_source_alias, request.incoming_via) {
                (Some(alias), Some(via)) => !via.contains_pseudonym(alias),
                _ => true,
            })
            .collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let request_ref = &request;
        let results: Vec<ForwardedWriteResult> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let mut path = vec!["ngsi-ld", "v1"];
                path.extend(request_ref.batch_path.split('/').filter(|segment| !segment.is_empty()));
                let url = forwarding_url(&csr.access.endpoint, &path, request_ref.query_string.unwrap_or(""));

                let exclusive_attrs = if csr.access.mode == Some(ForwardingMode::Exclusive) {
                    collect_exclusive_attrs(&csr)
                } else {
                    Vec::new()
                };

                match self
                    .forward_write(
                        url,
                        reqwest::Method::POST,
                        Some(request_ref.body),
                        request_ref.context_url,
                        csr.tenant.as_ref(),
                        request_ref.incoming_via,
                    )
                    .await
                {
                    Ok((status, response_body)) => {
                        let mode = csr.access.mode.unwrap_or_default();
                        let outcome = if status < 400 {
                            ForwardedWriteOutcome::Completed { status, body: response_body }
                        } else {
                            ForwardedWriteOutcome::RemoteHttpFailure { status, body: response_body }
                        };
                        Some(ForwardedWriteResult {
                            mode,
                            outcome,
                            exclusive_attrs,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to forward batch to context source {}: {}", csr.id, e);
                        Some(ForwardedWriteResult {
                            mode: csr.access.mode.unwrap_or_default(),
                            outcome: ForwardedWriteOutcome::TransportFailure { detail: e.to_string() },
                            exclusive_attrs,
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        if results
            .iter()
            .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) && result.is_failure())
        {
            return Err(FederationError::context_source_gateway(
                "An exclusive or redirect context source failed to complete the batch operation",
            ));
        }
        Ok(results)
    }

    /// Make a write request (POST/PUT/PATCH/DELETE) to a context source endpoint.
    async fn forward_write(
        &self,
        url: Result<Url, FederationError>,
        method: reqwest::Method,
        body: Option<&[u8]>,
        context_url: Option<&IriRefBuf>,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> Result<(u16, Option<Value>), FederationError> {
        let url = url?;
        debug!("CS forwarding {} {}", method, url);

        let mut request = self.build_request(method, &url, context_url, tenant, incoming_via);

        if let Some(body_bytes) = body {
            request = request.header("Content-Type", APPLICATION_LD_JSON.to_string()).body(body_bytes.to_vec());
        }

        let (status, _warning, bytes) = self.send_request(request).await?;

        let response_body = if bytes.is_empty() {
            None
        } else {
            let text = String::from_utf8_lossy(&bytes);
            serde_json::from_str(&text).ok()
        };

        Ok((status.as_u16(), response_body))
    }
}
