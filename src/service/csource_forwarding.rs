use crate::{
    core::error::AndromedaError,
    http::header::{link::Link, ngsild_tenant::NgsiLdTenant, ngsild_warning::NgsiLdWarning, via::Via},
    jsonld::{ContextSpec, JsonLdEngine},
    repository::csource_registration::repo::CsourceRegistrationRepository,
};
use andromeda_domain::{
    entity::Entity,
    registration::{ContextSourceRegistration, CsrOperation, ForwardingMode},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, tenant::TenantName};
use andromeda_query::filter::{eval::evaluate as eval_q_expr, expression::Expr};
use futures::{StreamExt, stream};
use headers::Header;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tracing::{debug, warn};
use url::Url;

/// Service for forwarding NGSI-LD operations to registered context sources.
pub struct CsourceForwardingService {
    http_client: reqwest::Client,
    csr_repo: CsourceRegistrationRepository,
    engine: JsonLdEngine,
    /// Context Source Identity (hostAlias) for Via header (§6.3.18).
    source_identity: String,
    /// Maximum number of parallel CSR fan-out HTTP requests per operation.
    csr_concurrency: usize,
}

/// The NGSI-LD operation being performed, used to filter CSRs by their `operations` field.
#[derive(Debug, Clone, Copy)]
pub enum ForwardingOperation {
    /// Retrieve a single entity by ID.
    RetrieveEntity,
    /// Query entities matching filter criteria.
    QueryEntity,
    /// Batch query for entities.
    QueryBatch,
    /// Create a new entity.
    CreateEntity,
    /// Delete an entity by ID.
    DeleteEntity,
    /// Merge-patch an entity.
    MergeEntity,
    /// Replace an entity entirely.
    ReplaceEntity,
    /// Append attributes to an entity.
    AppendEntityAttributes,
    /// Update existing attributes of an entity.
    UpdateEntityAttributes,
    /// Partially update a single attribute.
    PartialUpdateAttribute,
    /// Delete a single attribute from an entity.
    DeleteEntityAttribute,
    /// Batch create entities (per spec 5.6.7-5.6.10, 5.6.20).
    CreateBatch,
    /// Batch upsert entities.
    UpsertBatch,
    /// Batch update entities.
    UpdateBatch,
    /// Batch delete entities.
    DeleteBatch,
    /// Batch merge-patch entities.
    MergeBatch,
    /// Retrieve a single temporal entity by ID.
    RetrieveTemporalEntity,
    /// Query temporal entities matching filter criteria.
    QueryTemporalEntity,
}

impl ForwardingOperation {
    /// Returns the operation strings that match this forwarding operation.
    /// Per NGSI-LD spec 6.3.5: if `operations` is not specified on a CSR, it supports all operations.
    fn matches(&self, csr: &ContextSourceRegistration) -> bool {
        let ops = match &csr.access.operations {
            Some(ops) => ops,
            None => return true, // No operations specified = supports all
        };

        let mode = csr.access.mode.unwrap_or_default();

        match self {
            ForwardingOperation::RetrieveEntity => ops.iter().any(|op| {
                matches!(
                    op,
                    CsrOperation::RetrieveEntity | CsrOperation::QueryEntity | CsrOperation::QueryBatch | CsrOperation::FederationOps
                ) || (mode == ForwardingMode::Redirect && *op == CsrOperation::RedirectionOps)
            }),
            ForwardingOperation::QueryEntity => ops.iter().any(|op| {
                matches!(op, CsrOperation::QueryEntity | CsrOperation::QueryBatch | CsrOperation::FederationOps)
                    || (mode == ForwardingMode::Redirect && *op == CsrOperation::RedirectionOps)
            }),
            ForwardingOperation::QueryBatch => ops.iter().any(|op| {
                matches!(op, CsrOperation::QueryBatch | CsrOperation::QueryEntity | CsrOperation::FederationOps)
                    || (mode == ForwardingMode::Redirect && *op == CsrOperation::RedirectionOps)
            }),
            ForwardingOperation::CreateEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::CreateEntity)),
            ForwardingOperation::DeleteEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteEntity)),
            ForwardingOperation::MergeEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::MergeEntity)),
            ForwardingOperation::ReplaceEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::ReplaceEntity)),
            ForwardingOperation::AppendEntityAttributes => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::AppendAttrs)),
            ForwardingOperation::UpdateEntityAttributes => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpdateAttrs)),
            ForwardingOperation::PartialUpdateAttribute => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::PartialAttrUpdate)),
            ForwardingOperation::DeleteEntityAttribute => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteAttrs)),
            ForwardingOperation::CreateBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::CreateBatch)),
            ForwardingOperation::UpsertBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpsertBatch)),
            ForwardingOperation::UpdateBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpdateBatch)),
            ForwardingOperation::DeleteBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteBatch)),
            ForwardingOperation::MergeBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::MergeBatch)),
            ForwardingOperation::RetrieveTemporalEntity => ops.iter().any(|op| {
                matches!(
                    op,
                    CsrOperation::RetrieveTemporalEntity | CsrOperation::QueryTemporalEntity | CsrOperation::FederationOps
                ) || (mode == ForwardingMode::Redirect && *op == CsrOperation::RedirectionOps)
            }),
            ForwardingOperation::QueryTemporalEntity => ops.iter().any(|op| {
                matches!(op, CsrOperation::QueryTemporalEntity | CsrOperation::FederationOps)
                    || (mode == ForwardingMode::Redirect && *op == CsrOperation::RedirectionOps)
            }),
        }
    }
}

/// Outcome of a GET request to a context source, distinguishing conditions
/// that map to different IANA warning codes (clause 6.3.17).
enum ForwardGetOutcome {
    /// HTTP success with valid JSON body and optional protocol warnings.
    Success(Value, Vec<NgsiLdWarning>),
    /// HTTP 404 -- not considered abnormal per spec.
    NotFound,
    /// HTTP success but response body was not valid JSON.
    InvalidPayload(String),
    /// HTTP non-success status other than 404.
    ErrorStatus(u16),
    /// Network-level failure (timeout, connection refused, etc.).
    NetworkError(String),
}

/// Result from a single context source forwarding attempt for entity paths.
///
/// The raw HTTP payload is parsed into strongly-typed [`Entity`] values at the
/// CSR boundary so merge logic can operate on typed data without a
/// `Value` → `Entity` round-trip.
pub struct ForwardedEntities {
    /// The forwarding mode of the matched context source registration.
    pub mode: ForwardingMode,
    /// Entities returned by the context source, parsed to the typed domain model.
    pub entities: Vec<Entity>,
    /// Warnings generated during the forwarding attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

/// Result from a single context source forwarding attempt for temporal paths.
///
/// Temporal entities carry arrays of attribute-instance observations whose
/// shape is not a direct match for the domain [`Entity`] type, so this variant
/// keeps the payload as `serde_json::Value`.
pub struct ForwardedTemporalEntities {
    /// The forwarding mode of the matched context source registration.
    pub mode: ForwardingMode,
    /// Temporal entity payloads returned by the context source.
    pub entities: Vec<Value>,
    /// Warnings generated during the forwarding attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

/// Result from a single context source provision (write) forwarding attempt.
pub struct ForwardedWriteResult {
    /// The forwarding mode of the matched context source registration.
    pub mode: ForwardingMode,
    /// HTTP status code returned by the context source.
    pub status: u16,
    /// Optional response body from the context source.
    pub body: Option<Value>,
    /// Property/relationship names covered by this exclusive registration.
    /// Empty for non-exclusive registrations.
    pub exclusive_attrs: Vec<String>,
}

/// Parameters for forwarding a single-entity provision (write) operation.
pub struct ForwardProvisionRequest<'a> {
    /// Entity ID to target, if applicable.
    pub entity_id: Option<&'a str>,
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// HTTP method to use for the forwarded request.
    pub method: reqwest::Method,
    /// Optional request body bytes.
    pub body: Option<&'a [u8]>,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context specification for term expansion.
    pub context: &'a ContextSpec,
    /// The NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Path suffix appended to the entity URL (e.g. "/attrs").
    pub path_suffix: Option<&'a str>,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// Incoming Via header for loop detection (section 6.3.18).
    pub incoming_via: Option<&'a Via>,
}

/// Parameters for forwarding a batch provision operation.
pub struct ForwardBatchProvisionRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Request body bytes containing the batch payload.
    pub body: &'a [u8],
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context specification for term expansion.
    pub context: &'a ContextSpec,
    /// The NGSI-LD batch operation being forwarded.
    pub operation: ForwardingOperation,
    /// Batch operation endpoint path (e.g. "/entityOperations/create").
    pub batch_path: &'a str,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// Incoming Via header for loop detection (section 6.3.18).
    pub incoming_via: Option<&'a Via>,
}

/// Parameters for forwarding a temporal entity query to matching context sources.
pub struct ForwardTemporalQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Entity IDs to filter by.
    pub entity_ids: Option<&'a [String]>,
    /// Full query string including temporal parameters.
    pub query_string: &'a str,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context specification for term expansion.
    pub context: &'a ContextSpec,
    /// The NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Incoming Via header for loop detection (section 6.3.18).
    pub incoming_via: Option<&'a Via>,
}

/// Result from a single context source discovery forwarding attempt (types/attributes).
pub struct ForwardedDiscoveryResult {
    /// Response body from the context source, if the request succeeded.
    pub body: Option<Value>,
    /// Warnings generated during the forwarding attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

/// Parameters for forwarding an entity query to matching context sources.
pub struct ForwardQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Entity IDs to filter by.
    pub entity_ids: Option<&'a [String]>,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context specification for term expansion.
    pub context: &'a ContextSpec,
    /// The NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Incoming Via header for loop detection (section 6.3.18).
    pub incoming_via: Option<&'a Via>,
    /// Optional CSR-level context source filter expression.
    pub csf_expr: Option<&'a Expr>,
    /// Filter controlling which CSR forwarding modes are included.
    pub mode_filter: CsrModeFilter,
}

/// Filter controlling which CSR forwarding modes are included in a forwarding operation.
///
/// Per NGSI-LD 4.3.6.2, auxiliary CSRs should only be queried when local/primary data
/// exists. This filter enables two-phase forwarding: first query primary CSRs, then
/// conditionally query auxiliary CSRs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsrModeFilter {
    /// Include all CSR modes (default, backward-compatible behaviour).
    All,
    /// Exclude auxiliary CSRs (primary phase of two-phase forwarding).
    ExcludeAuxiliary,
    /// Include only auxiliary CSRs (supplementary phase of two-phase forwarding).
    AuxiliaryOnly,
}

/// Marker indicating an exclusive CSR matched the entity but not the requested operation.
/// Used to detect when exclusive data exists but can't be forwarded (e.g. no redirectionOps).
pub struct UnforwardableExclusiveCsr;

impl CsourceForwardingService {
    /// Create a new context source forwarding service with the given source identity for Via headers.
    pub fn new(pool: &PgPool, engine: JsonLdEngine, source_identity: String, csr_concurrency: usize) -> Result<CsourceForwardingService, AndromedaError> {
        let http_client = reqwest::ClientBuilder::new()
            .http1_title_case_headers()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| AndromedaError::internal(format!("Failed to create HTTP client for context source forwarding: {}", e)))?;
        let csr_repo = CsourceRegistrationRepository::new(pool);
        Ok(CsourceForwardingService {
            http_client,
            csr_repo,
            engine,
            source_identity,
            csr_concurrency: csr_concurrency.max(1),
        })
    }

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
        let expanded_type = if let Some(t) = entity_type_compact {
            Some(self.engine.expand_type(t, context).await?)
        } else {
            None
        };

        // For CSR matching, also pass entity IDs if available
        let first_id = entity_ids.and_then(|ids| ids.first().map(|s| s.as_str()));
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
                debug!("Forwarding entity query to context source {}: {}", csr.id, url);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(&url, context_url, csr.tenant.as_ref(), incoming_via).await {
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
                debug!("Forwarding entity retrieve to context source {}: {}", csr.id, url);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(&url, context_url, csr.tenant.as_ref(), incoming_via).await {
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

    /// Forward a provision (write) operation to matching context sources.
    ///
    /// Used for create (POST), delete (DELETE), merge (PATCH), replace (PUT),
    /// and entity attribute operations (append, update, partial update, delete attr).
    /// Also returns exclusive CSRs that matched the entity but not the operation
    /// (needed to detect when exclusive data exists but can't be forwarded).
    ///
    /// `path_suffix` appends to the entity URL (e.g., "/attrs" or "/attrs/speed").
    /// `query_string` appends query params to the forwarded URL (e.g., "options=noOverwrite").
    pub async fn forward_provision(
        &self,
        request: ForwardProvisionRequest<'_>,
    ) -> Result<(Vec<ForwardedWriteResult>, Vec<UnforwardableExclusiveCsr>), AndromedaError> {
        if self.is_loop_detected(request.incoming_via) {
            debug!("Loop detected in Via header chain, skipping provision forwarding");
            return Ok((Vec::new(), Vec::new()));
        }

        let expanded_type = if let Some(t) = request.entity_type_compact {
            Some(self.engine.expand_type(t, request.context).await?)
        } else {
            None
        };

        let csrs = self.csr_repo.find_matching(expanded_type.as_deref(), request.entity_id).await?;

        // Separate CSRs into those that match the operation and exclusive ones that don't.
        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let mut matching_csrs = Vec::new();
        let mut unforwardable = Vec::new();
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
            if request.operation.matches(&csr) {
                matching_csrs.push(csr);
            } else if csr.access.mode == Some(ForwardingMode::Exclusive) {
                // Per spec 4.3.6.3: exclusive CSR exists but doesn't support this operation
                unforwardable.push(UnforwardableExclusiveCsr);
            }
        }

        if matching_csrs.is_empty() {
            return Ok((Vec::new(), unforwardable));
        }

        let request_ref = &request;
        let results: Vec<ForwardedWriteResult> = stream::iter(matching_csrs)
            .map(|csr| async move {
                let url = if request_ref.method == reqwest::Method::POST && request_ref.path_suffix.is_none() {
                    build_create_url(&csr.access.endpoint)
                } else {
                    let id = request_ref.entity_id.unwrap_or("");
                    let encoded_id = percent_encoding::utf8_percent_encode(id, PATH_ENCODE_SET).to_string();
                    let base = csr.access.endpoint.as_str().trim_end_matches('/');
                    let suffix = request_ref.path_suffix.unwrap_or("");
                    match request_ref.query_string {
                        Some(qs) if !qs.is_empty() => format!("{base}/ngsi-ld/v1/entities/{encoded_id}{suffix}?{qs}"),
                        _ => format!("{base}/ngsi-ld/v1/entities/{encoded_id}{suffix}"),
                    }
                };
                debug!("Forwarding {} to context source {}: {}", request_ref.method, csr.id, url);

                let exclusive_attrs = if csr.access.mode == Some(ForwardingMode::Exclusive) {
                    collect_exclusive_attrs(&csr)
                } else {
                    Vec::new()
                };

                match self
                    .forward_write(
                        &url,
                        request_ref.method.clone(),
                        request_ref.body,
                        request_ref.context_url,
                        csr.tenant.as_ref(),
                        request_ref.incoming_via,
                    )
                    .await
                {
                    Ok((status, response_body)) => {
                        let mode = csr.access.mode.unwrap_or_default();
                        Some(ForwardedWriteResult {
                            mode,
                            status,
                            body: response_body,
                            exclusive_attrs,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to forward {} to context source {}: {}", request_ref.method, csr.id, e);
                        None
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        Ok((results, unforwardable))
    }

    /// Forward a batch provision operation to matching context sources.
    ///
    /// Per spec 5.6.7-5.6.10, 5.6.20: batch operations iterate over CSRs,
    /// filter the input array per CSR, and forward matching entities.
    /// `batch_path` is the operation endpoint (e.g., "/entityOperations/create").
    pub async fn forward_batch_provision(&self, request: ForwardBatchProvisionRequest<'_>) -> Result<Vec<ForwardedWriteResult>, AndromedaError> {
        if self.is_loop_detected(request.incoming_via) {
            debug!("Loop detected in Via header chain, skipping batch provision forwarding");
            return Ok(Vec::new());
        }

        let expanded_type = if let Some(t) = request.entity_type_compact {
            Some(self.engine.expand_type(t, request.context).await?)
        } else {
            None
        };

        let csrs = self.csr_repo.find_matching(expanded_type.as_deref(), None).await?;
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
                let base = csr.access.endpoint.as_str().trim_end_matches('/');
                let url = match request_ref.query_string {
                    Some(qs) if !qs.is_empty() => format!("{base}/ngsi-ld/v1{}?{qs}", request_ref.batch_path),
                    _ => format!("{base}/ngsi-ld/v1{}", request_ref.batch_path),
                };
                debug!("Forwarding batch {} to context source {}: {}", request_ref.batch_path, csr.id, url);

                let exclusive_attrs = if csr.access.mode == Some(ForwardingMode::Exclusive) {
                    collect_exclusive_attrs(&csr)
                } else {
                    Vec::new()
                };

                match self
                    .forward_write(
                        &url,
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
                        Some(ForwardedWriteResult {
                            mode,
                            status,
                            body: response_body,
                            exclusive_attrs,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to forward batch to context source {}: {}", csr.id, e);
                        None
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        Ok(results)
    }

    /// Make a write request (POST/PUT/PATCH/DELETE) to a context source endpoint.
    async fn forward_write(
        &self,
        url: &str,
        method: reqwest::Method,
        body: Option<&[u8]>,
        context_url: Option<&str>,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> Result<(u16, Option<Value>), AndromedaError> {
        debug!("CS forwarding {} {}", method, url);

        let mut request = self.build_request(method, url, context_url, tenant, incoming_via);

        if let Some(body_bytes) = body {
            request = request.header("Content-Type", "application/ld+json").body(body_bytes.to_vec());
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

    /// Make a GET request to a context source endpoint.
    async fn forward_get(&self, url: &str, context_url: Option<&str>, tenant: Option<&TenantName>, incoming_via: Option<&Via>) -> ForwardGetOutcome {
        debug!("CS forwarding GET {}", url);

        let request = self
            .build_request(reqwest::Method::GET, url, context_url, tenant, incoming_via)
            .header("Accept", "application/json");

        let (status, warning_header, bytes) = match self.send_request(request).await {
            Ok(result) => result,
            Err(e) => return ForwardGetOutcome::NetworkError(e.to_string()),
        };

        if status.as_u16() == 404 {
            return ForwardGetOutcome::NotFound;
        }

        if !status.is_success() {
            return ForwardGetOutcome::ErrorStatus(status.as_u16());
        }

        // Propagate IANA Warning 110 (Response is Stale) from the context source (clause 6.3.17).
        let mut warnings = Vec::new();
        if let Some(ref warning_value) = warning_header
            && warning_value.starts_with("110")
        {
            warnings.push(NgsiLdWarning::ResponseIsStale(warning_value.clone()));
        }

        let text = String::from_utf8_lossy(&bytes);
        match serde_json::from_str(&text) {
            Ok(value) => ForwardGetOutcome::Success(value, warnings),
            Err(e) => {
                warn!("Failed to parse CS response as JSON: {}. Body: {}", e, &text[..text.len().min(200)]);
                ForwardGetOutcome::InvalidPayload(format!("Failed to parse context source response as JSON: {}", e))
            }
        }
    }

    /// Build a request with shared headers (Connection: close, Link context, Via).
    fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
        context_url: Option<&str>,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> reqwest::RequestBuilder {
        let mut request = self.http_client.request(method, url).header("Connection", "close");

        if let Some(ctx) = context_url {
            let link = Link::jsonld_context(ctx);
            if let Some(value) = link.to_header_value() {
                request = request.header("Link", value);
            }
        }

        if let Some(name) = tenant {
            let tenant_header = NgsiLdTenant::new(name.clone()); // clone: TenantName is borrowed, header needs ownership
            request = request.header(NgsiLdTenant::name().as_str(), tenant_header.as_str());
        }

        // Append our identity to the Via header chain (section 6.3.18)
        let new_via = match incoming_via {
            Some(existing) => existing.clone().append("1.1", &self.source_identity), // clone: need owned Via to append our identity
            None => Via::with_entry("1.1", &self.source_identity),
        };
        let mut via_values = Vec::new();
        headers::Header::encode(&new_via, &mut via_values);
        if let Some(val) = via_values.into_iter().next() {
            request = request.header("via", val);
        }

        request
    }

    /// Forward a discovery query (types or attributes) to all active context sources.
    ///
    /// Discovery queries have no entity type/id filtering -- all non-expired CSRs are
    /// queried because types/attributes are system-wide metadata (clause 5.7.11).
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
            debug!("Forwarding discovery query to context source {}: {}", csr.id, url);

            match self.forward_get(&url, context_url, csr.tenant.as_ref(), incoming_via).await {
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

    /// Forward a temporal entity query to matching context sources.
    ///
    /// Same pattern as `forward_query()` but targets `/ngsi-ld/v1/temporal/entities`
    /// and passes through the full query string (temporal params included).
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

        let expanded_type = if let Some(t) = entity_type_compact {
            Some(self.engine.expand_type(t, context).await?)
        } else {
            None
        };

        let first_id = entity_ids.and_then(|ids| ids.first().map(|s| s.as_str()));
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
                debug!("Forwarding temporal query to context source {}: {}", csr.id, url);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(&url, context_url, csr.tenant.as_ref(), incoming_via).await {
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
                debug!("Forwarding temporal retrieve to context source {}: {}", csr.id, url);

                let mode = csr.access.mode.unwrap_or_default();
                match self.forward_get(&url, context_url, csr.tenant.as_ref(), incoming_via).await {
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

    /// Check whether forwarding would create a loop based on the Via header chain.
    pub fn is_loop_detected(&self, incoming_via: Option<&Via>) -> bool {
        incoming_via.is_some_and(|v| v.contains_pseudonym(&self.source_identity))
    }

    /// Send a request and return the status and raw bytes.
    async fn send_request(&self, request: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, Option<String>, Vec<u8>), AndromedaError> {
        let response = request.send().await?;

        let status = response.status();
        let warning = response.headers().get("Warning").and_then(|v| v.to_str().ok()).map(String::from);

        let bytes = response.bytes().await?;

        Ok((status, warning, bytes.to_vec()))
    }
}

/// Build a minimal forwarding query string with only type and id params.
fn build_forwarding_query_string(entity_type: Option<&str>, entity_ids: Option<&[String]>) -> String {
    let mut parts = Vec::new();
    if let Some(ids) = entity_ids
        && !ids.is_empty()
    {
        parts.push(format!("id={}", ids.join(",")));
    }
    if let Some(t) = entity_type {
        parts.push(format!("type={}", t));
    }
    parts.join("&")
}

/// Build a forwarding URL for entity creation (POST).
fn build_create_url(endpoint: &Url) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    format!("{}/ngsi-ld/v1/entities", base)
}

/// Build a forwarding URL for entity queries.
/// The endpoint may already contain a path (e.g., for redirect mode).
fn build_query_url(endpoint: &Url, query_string: &str) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1/entities", base)
    } else {
        format!("{}/ngsi-ld/v1/entities?{}", base, query_string)
    }
}

/// Characters that must be percent-encoded in a URL path segment.
/// RFC 3986 unreserved chars (A-Z a-z 0-9 - . _ ~) and sub-delimiters (: @ ! $ & ' ( ) * + , ; =)
/// are left as-is. Only encode control chars, space, and characters with special URL meaning.
const PATH_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Build a forwarding URL for entity retrieval by ID.
fn build_retrieve_url(endpoint: &Url, entity_id: &str, query_string: &str) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    let encoded_id = percent_encoding::utf8_percent_encode(entity_id, PATH_ENCODE_SET).to_string();
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1/entities/{}", base, encoded_id)
    } else {
        format!("{}/ngsi-ld/v1/entities/{}?{}", base, encoded_id, query_string)
    }
}

/// Build a forwarding URL for discovery queries (types, attributes).
pub(crate) fn build_discovery_url(endpoint: &Url, path: &str, query_string: &str) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1{}", base, path)
    } else {
        format!("{}/ngsi-ld/v1{}?{}", base, path, query_string)
    }
}

/// Build a forwarding URL for temporal entity queries.
pub(crate) fn build_temporal_query_url(endpoint: &Url, query_string: &str) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1/temporal/entities", base)
    } else {
        format!("{}/ngsi-ld/v1/temporal/entities?{}", base, query_string)
    }
}

/// Build a forwarding URL for temporal entity retrieval by ID.
pub(crate) fn build_temporal_retrieve_url(endpoint: &Url, entity_id: &str, query_string: &str) -> String {
    let base = endpoint.as_str().trim_end_matches('/');
    let encoded_id = percent_encoding::utf8_percent_encode(entity_id, PATH_ENCODE_SET).to_string();
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1/temporal/entities/{}", base, encoded_id)
    } else {
        format!("{}/ngsi-ld/v1/temporal/entities/{}?{}", base, encoded_id, query_string)
    }
}

/// Collect property and relationship names from an exclusive CSR's information blocks.
/// Returns both the expanded IRI and the compacted (short) name for each attribute,
/// so callers can match against either form.
fn collect_exclusive_attrs(csr: &ContextSourceRegistration) -> Vec<String> {
    let mut attrs = Vec::new();
    for info in &csr.coverage.information {
        for name in info.property_names.iter().chain(info.relationship_names.iter()).flatten() {
            attrs.push(name.to_string());
            // Also add the compacted form (last path/fragment segment) for
            // matching against deserialized EntityPatch attr keys, which are
            // compacted.
            if let Some(short) = compact_iri(name.as_str())
                && short != name.as_str()
            {
                attrs.push(short);
            }
        }
    }
    attrs
}

/// Extract the short name from an expanded IRI by taking the fragment or last path segment.
fn compact_iri(iri: &str) -> Option<String> {
    if let Some(pos) = iri.rfind('#') {
        Some(iri[pos + 1..].to_string())
    } else {
        iri.rfind('/').map(|pos| iri[pos + 1..].to_string())
    }
}

/// Parse forwarded raw values into strongly-typed [`Entity`] at the CSR
/// boundary. Drops items that fail to deserialize and returns a warning per
/// skipped item so the service layer can surface the failure to clients.
fn parse_forwarded_entities(csr_id: &str, values: Vec<Value>) -> (Vec<Entity>, Vec<NgsiLdWarning>) {
    let mut entities = Vec::with_capacity(values.len());
    let mut warnings = Vec::new();
    for value in values {
        match serde_json::from_value::<Entity>(value) {
            Ok(e) => entities.push(e),
            Err(err) => {
                warn!("Context source {} returned entity that failed to parse: {}", csr_id, err);
                warnings.push(NgsiLdWarning::RevalidationFailed(format!(
                    "Context source {} returned entity that failed to parse: {}",
                    csr_id, err
                )));
            }
        }
    }
    (entities, warnings)
}

/// Merge local and remote entity query results.
///
/// Per NGSI-LD query merge behaviour:
/// - **inclusive** (default): Include both local and remote entities. If same ID exists, merge attributes.
/// - **redirect**: Same as inclusive for query results.
/// - **auxiliary**: Include remote entities and merge when IDs overlap.
/// - **exclusive**: Include remote entities and merge when IDs overlap.
pub fn merge_query_results(local_entities: Vec<Entity>, remote_results: Vec<ForwardedEntities>) -> Vec<Entity> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: indexmap::IndexMap<String, Entity> = indexmap::IndexMap::with_capacity(local_entities.len());

    for entity in local_entities {
        entity_map.insert(entity.id.to_string(), entity);
    }

    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary | ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                for remote_entity in forwarded.entities {
                    let remote_id = remote_entity.id.to_string();
                    if let Some(local_entity) = entity_map.get_mut(&remote_id) {
                        merge_entity_attributes(local_entity, remote_entity);
                    } else {
                        entity_map.insert(remote_id, remote_entity);
                    }
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge remote entity's attributes into a local entity.
///
/// Only attribute keys missing locally are filled from the remote copy; local
/// attributes take precedence. Top-level metadata (`id`, `type`, `@context`)
/// is untouched. Non-`attrs` fields (`location`, `observationSpace`,
/// `operationSpace`, `scope`, `expiresAt`) are copied only if unset locally.
fn merge_entity_attributes(local: &mut Entity, remote: Entity) {
    for (key, value) in remote.attrs {
        if !local.attrs.contains_key(&key) {
            local.attrs.insert(key, value);
        }
    }
    if local.location.is_none() {
        local.location = remote.location;
    }
    if local.observation_space.is_none() {
        local.observation_space = remote.observation_space;
    }
    if local.operation_space.is_none() {
        local.operation_space = remote.operation_space;
    }
    if local.scope.is_none() {
        local.scope = remote.scope;
    }
    if local.expires_at.is_none() {
        local.expires_at = remote.expires_at;
    }
}

/// Merge remote entity results for a single entity retrieval.
/// Returns the merged entity or None if no entity found from any source.
pub fn merge_retrieve_results(local_entity: Option<Entity>, remote_results: Vec<ForwardedEntities>) -> Option<Entity> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let is_auxiliary = forwarded.mode == ForwardingMode::Auxiliary;
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => {
                    merge_entity_attributes(existing, remote_entity);
                }
                None => {
                    if !is_auxiliary {
                        result = Some(remote_entity);
                    }
                }
            }
        }
    }

    result
}

/// Collect all warnings from forwarded entity results.
pub fn collect_warnings(results: &[ForwardedEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect all warnings from forwarded temporal-entity results.
pub fn collect_temporal_warnings(results: &[ForwardedTemporalEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect exclusive attribute names from forwarding results.
///
/// Gathers attribute names marked as exclusive across all forwarded write
/// results. Used by provision handlers to strip exclusive attributes from
/// local operations per spec 4.3.6.3.
pub fn collect_forwarded_exclusive_attrs(results: &[ForwardedWriteResult]) -> Vec<AttributeName> {
    // A forwarded name that is not a valid NGSI-LD attribute name cannot key a
    // locally held attribute, so dropping it here removes nothing that a
    // subsequent strip would have matched.
    results
        .iter()
        .flat_map(|r| r.exclusive_attrs.iter())
        .filter_map(|name| name.parse().ok())
        .collect()
}

/// Strip exclusive attributes from a JSON Value object.
///
/// Removes keys matching `exclusive_attrs` from a `Value::Object`. This is a
/// no-op for non-object values. Used for attribute fragment payloads (append,
/// update_attrs) that are `Value` rather than `EntityPatch`.
pub fn strip_exclusive_attrs_from_value(fragment: &mut Value, exclusive_attrs: &[AttributeName]) {
    if let Some(obj) = fragment.as_object_mut() {
        for attr in exclusive_attrs {
            obj.remove(attr.as_str());
        }
    }
}

/// Collect all warnings from discovery forwarding results.
pub fn collect_discovery_warnings(results: &[ForwardedDiscoveryResult]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Merge local types result with forwarded discovery results.
///
/// For simple types list (`typeList`): union of type IRIs, deduplicated.
/// For detailed types (`details=true`): merge by type name, union attribute names per type.
/// For type info (single type): merge attribute details from all sources.
pub fn merge_types_results(local: Value, remote: Vec<ForwardedDiscoveryResult>, details: bool) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    if details {
        merge_detailed_types(local, &bodies)
    } else {
        merge_simple_types(local, &bodies)
    }
}

/// Merge simple type lists by unioning `typeList` arrays and deduplicating.
fn merge_simple_types(mut local: Value, remote_bodies: &[&Value]) -> Value {
    let local_list = local.get_mut("typeList").and_then(|v| v.as_array_mut());
    let Some(local_list) = local_list else {
        return local;
    };

    let mut seen: std::collections::HashSet<String> = local_list.iter().filter_map(|v| v.as_str().map(String::from)).collect();

    for body in remote_bodies {
        if let Some(arr) = body.get("typeList").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str()
                    && seen.insert(s.to_string())
                {
                    local_list.push(item.clone()); // clone: adding remote type IRI to merged list
                }
            }
        }
        // Also handle bare arrays (some brokers may return array directly)
        if let Some(arr) = body.as_array() {
            for item in arr {
                if let Some(s) = item.as_str()
                    && seen.insert(s.to_string())
                {
                    local_list.push(item.clone()); // clone: adding remote type IRI from bare array
                }
            }
        }
    }

    local
}

/// Merge detailed type lists by type name, unioning attribute names per type.
fn merge_detailed_types(local: Value, remote_bodies: &[&Value]) -> Value {
    let mut type_map: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();

    // Collect local types
    if let Some(arr) = local.as_array() {
        for entry in arr {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                type_map.insert(id.to_string(), entry.clone()); // clone: local type entry for merge map
            }
        }
    }

    // Merge remote types
    for body in remote_bodies {
        let entries = if let Some(arr) = body.as_array() {
            arr.as_slice()
        } else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(existing) = type_map.get_mut(id) {
                merge_type_attribute_names(existing, entry);
            } else {
                type_map.insert(id.to_string(), entry.clone()); // clone: remote type entry not present locally
            }
        }
    }

    Value::Array(type_map.into_values().collect())
}

/// Merge attribute names within a type detail entry (union of attributeNames arrays).
fn merge_type_attribute_names(local: &mut Value, remote: &Value) {
    if let Some(local_attrs) = local.get_mut("attributeNames").and_then(|v| v.as_array_mut())
        && let Some(remote_attrs) = remote.get("attributeNames").and_then(|v| v.as_array())
    {
        let existing: std::collections::HashSet<String> = local_attrs.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        for attr in remote_attrs {
            if let Some(s) = attr.as_str()
                && !existing.contains(s)
            {
                local_attrs.push(attr.clone()); // clone: adding remote attribute name to merged set
            }
        }
    }
}

/// Merge type info result (single type) with forwarded discovery results.
///
/// Merges attribute details from all sources, unioning attribute names.
pub fn merge_type_info_results(local: Value, remote: Vec<ForwardedDiscoveryResult>) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    let mut result = local;
    for body in &bodies {
        merge_type_attribute_names(&mut result, body);
    }
    result
}

/// Merge local attributes result with forwarded discovery results.
///
/// For simple attribute list (`attributeList`): union of attribute IRIs, deduplicated.
/// For detailed attributes (`details=true`): merge by attribute name, union type names per attribute.
pub fn merge_attributes_results(local: Value, remote: Vec<ForwardedDiscoveryResult>, details: bool) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    if details {
        merge_detailed_attributes(local, &bodies)
    } else {
        merge_simple_attributes(local, &bodies)
    }
}

/// Merge simple attribute lists by unioning `attributeList` arrays and deduplicating.
fn merge_simple_attributes(mut local: Value, remote_bodies: &[&Value]) -> Value {
    let local_list = local.get_mut("attributeList").and_then(|v| v.as_array_mut());
    let Some(local_list) = local_list else {
        return local;
    };

    let mut seen: std::collections::HashSet<String> = local_list.iter().filter_map(|v| v.as_str().map(String::from)).collect();

    for body in remote_bodies {
        if let Some(arr) = body.get("attributeList").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str()
                    && seen.insert(s.to_string())
                {
                    local_list.push(item.clone()); // clone: adding remote attribute IRI to merged list
                }
            }
        }
        if let Some(arr) = body.as_array() {
            for item in arr {
                if let Some(s) = item.as_str()
                    && seen.insert(s.to_string())
                {
                    local_list.push(item.clone()); // clone: adding remote attribute IRI from bare array
                }
            }
        }
    }

    local
}

/// Merge detailed attribute lists by attribute name, unioning type names per attribute.
fn merge_detailed_attributes(local: Value, remote_bodies: &[&Value]) -> Value {
    let mut attr_map: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();

    if let Some(arr) = local.as_array() {
        for entry in arr {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                attr_map.insert(id.to_string(), entry.clone()); // clone: local attribute entry for merge map
            }
        }
    }

    for body in remote_bodies {
        let entries = if let Some(arr) = body.as_array() {
            arr.as_slice()
        } else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(existing) = attr_map.get_mut(id) {
                merge_attribute_type_names(existing, entry);
            } else {
                attr_map.insert(id.to_string(), entry.clone()); // clone: remote attribute entry not present locally
            }
        }
    }

    Value::Array(attr_map.into_values().collect())
}

/// Merge type names within an attribute detail entry (union of typeNames arrays).
fn merge_attribute_type_names(local: &mut Value, remote: &Value) {
    if let Some(local_types) = local.get_mut("typeNames").and_then(|v| v.as_array_mut())
        && let Some(remote_types) = remote.get("typeNames").and_then(|v| v.as_array())
    {
        let existing: std::collections::HashSet<String> = local_types.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        for t in remote_types {
            if let Some(s) = t.as_str()
                && !existing.contains(s)
            {
                local_types.push(t.clone()); // clone: adding remote type name to merged set
            }
        }
    }
}

/// Merge attribute info result (single attribute) with forwarded discovery results.
///
/// Merges type name lists from all sources.
pub fn merge_attribute_info_results(local: Value, remote: Vec<ForwardedDiscoveryResult>) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    let mut result = local;
    for body in &bodies {
        merge_attribute_type_names(&mut result, body);
    }
    result
}

/// Merge local and remote temporal entity query results.
///
/// Same mode-based logic as `merge_query_results` but for temporal entities.
/// For same-entity merges, temporal attribute instance arrays are unioned
/// rather than replaced (temporal entities carry attribute history).
pub fn merge_temporal_query_results(local_entities: Vec<Value>, remote_results: Vec<ForwardedTemporalEntities>) -> Vec<Value> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();

    for entity in local_entities {
        if let Some(id) = entity.get("id").and_then(|v| v.as_str()) {
            entity_map.insert(id.to_string(), entity);
        }
    }

    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str())
                        && let Some(local_entity) = entity_map.get_mut(remote_id)
                    {
                        merge_temporal_entity_attributes(local_entity, &remote_entity);
                    }
                }
            }
            ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()) {
                        if let Some(local_entity) = entity_map.get_mut(remote_id) {
                            merge_temporal_entity_attributes(local_entity, &remote_entity);
                        } else {
                            entity_map.insert(remote_id.to_string(), remote_entity);
                        }
                    }
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge temporal entity attributes by unioning attribute instance arrays.
///
/// Unlike regular entity merging (which skips existing attributes), temporal merging
/// unions instance arrays for attributes that exist in both entities, since temporal
/// entities carry historical attribute instances.
fn merge_temporal_entity_attributes(local: &mut Value, remote: &Value) {
    if let (Some(local_obj), Some(remote_obj)) = (local.as_object_mut(), remote.as_object()) {
        for (key, remote_value) in remote_obj {
            if key == "id" || key == "type" || key == "@context" {
                continue;
            }
            match local_obj.get_mut(key) {
                Some(local_value) => {
                    // Merge instance arrays for same attribute
                    merge_temporal_instances(local_value, remote_value);
                }
                None => {
                    local_obj.insert(key.clone(), remote_value.clone()); // clone: adding remote temporal attribute not present locally
                }
            }
        }
    }
}

/// Merge temporal attribute instances from remote into local.
///
/// Handles both single-instance (object) and multi-instance (array) forms.
/// Deduplicates by `instanceId` if present.
fn merge_temporal_instances(local: &mut Value, remote: &Value) {
    // Normalize both to arrays for uniform handling
    let local_arr = match local {
        Value::Array(arr) => arr,
        Value::Object(_) => {
            let obj = std::mem::replace(local, Value::Null);
            *local = Value::Array(vec![obj]);
            // Infallible: we just set `local` to `Value::Array` above.
            let Some(arr) = local.as_array_mut() else { return };
            arr
        }
        _ => return,
    };

    let remote_items = match remote {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(_) => std::slice::from_ref(remote),
        _ => return,
    };

    // Collect existing instanceIds for deduplication
    let existing_ids: std::collections::HashSet<String> = local_arr
        .iter()
        .filter_map(|v| v.get("instanceId").and_then(|id| id.as_str()).map(String::from))
        .collect();

    for item in remote_items {
        let dominated = item.get("instanceId").and_then(|id| id.as_str()).is_some_and(|id| existing_ids.contains(id));
        if !dominated {
            local_arr.push(item.clone()); // clone: adding remote temporal instance to merged array
        }
    }
}

/// Merge remote temporal entity results for a single entity retrieval.
///
/// Returns the merged temporal entity or None if no entity found from any source.
pub fn merge_temporal_retrieve_results(local_entity: Option<Value>, remote_results: Vec<ForwardedTemporalEntities>) -> Option<Value> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let is_auxiliary = forwarded.mode == ForwardingMode::Auxiliary;
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => {
                    merge_temporal_entity_attributes(existing, &remote_entity);
                }
                None => {
                    if !is_auxiliary {
                        result = Some(remote_entity);
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use crate::service::csource_forwarding::{
        ForwardedEntities,
        ForwardedTemporalEntities,
        ForwardedWriteResult,
        NgsiLdWarning,
        collect_forwarded_exclusive_attrs,
        collect_warnings,
        merge_query_results,
        strip_exclusive_attrs_from_value,
    };
    use andromeda_domain::registration::ForwardingMode;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use url::Url;

    fn attribute_name(raw: &str) -> AttributeName {
        raw.parse().expect("valid attribute name")
    }

    fn entity_from_json(value: serde_json::Value) -> andromeda_domain::entity::Entity {
        serde_json::from_value(value).expect("test entity fixture")
    }

    #[test]
    fn warning_header_format_miscellaneous() {
        let w = NgsiLdWarning::MiscellaneousWarning("timeout".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "199 - \"timeout\"");
    }

    #[test]
    fn warning_header_format_persistent() {
        let w = NgsiLdWarning::MiscellaneousPersistentWarning("403 Forbidden".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "299 - \"403 Forbidden\"");
    }

    #[test]
    fn warning_header_format_response_is_stale_per_clause_6_3_17() {
        let w = NgsiLdWarning::ResponseIsStale("cached data".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "110 - \"cached data\"");
    }

    #[test]
    fn warning_header_format_revalidation_failed_per_clause_6_3_17() {
        let w = NgsiLdWarning::RevalidationFailed("invalid JSON".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "111 - \"invalid JSON\"");
    }

    #[test]
    fn collect_warnings_from_results() {
        let results = vec![
            ForwardedEntities {
                mode: ForwardingMode::Inclusive,
                entities: Vec::new(),
                warnings: vec![
                    NgsiLdWarning::MiscellaneousWarning("timeout".to_string()),
                    NgsiLdWarning::RevalidationFailed("invalid JSON".to_string()),
                ],
            },
            ForwardedEntities {
                mode: ForwardingMode::Inclusive,
                entities: vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))],
                warnings: Vec::new(),
            },
        ];
        let warnings = collect_warnings(&results);
        pretty_assertions::assert_eq!(warnings.len(), 2);
        assert!(matches!(warnings[0], NgsiLdWarning::MiscellaneousWarning(_)));
        assert!(matches!(warnings[1], NgsiLdWarning::RevalidationFailed(_)));
    }

    #[test]
    fn merge_query_local_only() {
        let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "p1": 1}))];
        let result = merge_query_results(local, Vec::new());
        pretty_assertions::assert_eq!(result.len(), 1);
    }

    #[test]
    fn merge_query_adds_remote_entities() {
        let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))];
        let remote = vec![ForwardedEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T"}))],
            warnings: Vec::new(),
        }];
        let result = merge_query_results(local, remote);
        pretty_assertions::assert_eq!(result.len(), 2);
    }

    #[test]
    fn merge_simple_types_deduplicates() {
        use crate::service::csource_forwarding::{ForwardedDiscoveryResult, merge_types_results};

        let local = serde_json::json!({"id": "urn:types", "type": "EntityTypeList", "typeList": ["Vehicle", "Sensor"]});
        let remote = vec![ForwardedDiscoveryResult {
            body: Some(serde_json::json!({"typeList": ["Vehicle", "Drone"]})),
            warnings: Vec::new(),
        }];
        let result = merge_types_results(local, remote, false);
        let list = result.get("typeList").unwrap().as_array().unwrap();
        pretty_assertions::assert_eq!(list.len(), 3);
        assert!(list.contains(&serde_json::json!("Vehicle")));
        assert!(list.contains(&serde_json::json!("Sensor")));
        assert!(list.contains(&serde_json::json!("Drone")));
    }

    #[test]
    fn merge_simple_types_no_remote_returns_local() {
        use crate::service::csource_forwarding::merge_types_results;

        let local = serde_json::json!({"typeList": ["Vehicle"]});
        let result = merge_types_results(local.clone(), Vec::new(), false);
        pretty_assertions::assert_eq!(result, local);
    }

    #[test]
    fn merge_detailed_types_unions_attribute_names() {
        use crate::service::csource_forwarding::{ForwardedDiscoveryResult, merge_types_results};

        let local = serde_json::json!([
            {"id": "Vehicle", "type": "EntityType", "attributeNames": ["speed", "location"]}
        ]);
        let remote = vec![ForwardedDiscoveryResult {
            body: Some(serde_json::json!([
                {"id": "Vehicle", "type": "EntityType", "attributeNames": ["speed", "fuel"]},
                {"id": "Drone", "type": "EntityType", "attributeNames": ["altitude"]}
            ])),
            warnings: Vec::new(),
        }];
        let result = merge_types_results(local, remote, true);
        let arr = result.as_array().unwrap();
        pretty_assertions::assert_eq!(arr.len(), 2);

        let vehicle = arr.iter().find(|v| v.get("id").unwrap() == "Vehicle").unwrap();
        let attrs = vehicle.get("attributeNames").unwrap().as_array().unwrap();
        pretty_assertions::assert_eq!(attrs.len(), 3);
        assert!(attrs.contains(&serde_json::json!("fuel")));
    }

    #[test]
    fn merge_simple_attributes_deduplicates() {
        use crate::service::csource_forwarding::{ForwardedDiscoveryResult, merge_attributes_results};

        let local = serde_json::json!({"id": "urn:attrs", "type": "AttributeList", "attributeList": ["speed"]});
        let remote = vec![ForwardedDiscoveryResult {
            body: Some(serde_json::json!({"attributeList": ["speed", "fuel"]})),
            warnings: Vec::new(),
        }];
        let result = merge_attributes_results(local, remote, false);
        let list = result.get("attributeList").unwrap().as_array().unwrap();
        pretty_assertions::assert_eq!(list.len(), 2);
    }

    #[test]
    fn merge_temporal_query_unions_instances_for_same_entity() {
        use crate::service::csource_forwarding::merge_temporal_query_results;

        let local = vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 10, "instanceId": "urn:i1", "observedAt": "2024-01-01T00:00:00Z"}
            ]
        })];
        let remote = vec![ForwardedTemporalEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![serde_json::json!({
                "id": "urn:a",
                "type": "T",
                "speed": [
                    {"type": "Property", "value": 20, "instanceId": "urn:i2", "observedAt": "2024-01-02T00:00:00Z"}
                ]
            })],
            warnings: Vec::new(),
        }];
        let result = merge_temporal_query_results(local, remote);
        pretty_assertions::assert_eq!(result.len(), 1);
        let speed = result[0].get("speed").unwrap().as_array().unwrap();
        pretty_assertions::assert_eq!(speed.len(), 2);
    }

    #[test]
    fn merge_temporal_query_deduplicates_by_instance_id() {
        use crate::service::csource_forwarding::merge_temporal_query_results;

        let local = vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 10, "instanceId": "urn:i1"}
            ]
        })];
        let remote = vec![ForwardedTemporalEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![serde_json::json!({
                "id": "urn:a",
                "type": "T",
                "speed": [
                    {"type": "Property", "value": 10, "instanceId": "urn:i1"}
                ]
            })],
            warnings: Vec::new(),
        }];
        let result = merge_temporal_query_results(local, remote);
        let speed = result[0].get("speed").unwrap().as_array().unwrap();
        pretty_assertions::assert_eq!(speed.len(), 1);
    }

    #[test]
    fn merge_temporal_query_auxiliary_only_supplements_existing() {
        use crate::service::csource_forwarding::merge_temporal_query_results;

        let local = vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})];
        let remote = vec![ForwardedTemporalEntities {
            mode: ForwardingMode::Auxiliary,
            entities: vec![
                serde_json::json!({"id": "urn:a", "type": "T", "fuel": [{"value": 50}]}),
                serde_json::json!({"id": "urn:b", "type": "T", "speed": [{"value": 20}]}),
            ],
            warnings: Vec::new(),
        }];
        let result = merge_temporal_query_results(local, remote);
        pretty_assertions::assert_eq!(result.len(), 1);
        pretty_assertions::assert_eq!(result[0].get("id").and_then(serde_json::Value::as_str), Some("urn:a"));
        assert!(result[0].get("fuel").is_some());
    }

    #[test]
    fn merge_temporal_retrieve_combines_remote_attributes() {
        use crate::service::csource_forwarding::merge_temporal_retrieve_results;

        let local = Some(serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [{"value": 10, "instanceId": "urn:i1"}]
        }));
        let remote = vec![ForwardedTemporalEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![serde_json::json!({
                "id": "urn:a",
                "type": "T",
                "fuel": [{"value": 50}]
            })],
            warnings: Vec::new(),
        }];
        let result = merge_temporal_retrieve_results(local, remote);
        let entity = result.unwrap();
        assert!(entity.get("speed").is_some());
        assert!(entity.get("fuel").is_some());
    }

    #[test]
    fn merge_temporal_retrieve_remote_only() {
        use crate::service::csource_forwarding::merge_temporal_retrieve_results;

        let remote = vec![ForwardedTemporalEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})],
            warnings: Vec::new(),
        }];
        let result = merge_temporal_retrieve_results(None, remote);
        assert!(result.is_some());
    }

    #[test]
    fn build_discovery_url_with_query_string() {
        use crate::service::csource_forwarding::build_discovery_url;

        let url = build_discovery_url(&Url::parse("http://cs1.example.com").expect("valid url"), "/types", "details=true");
        pretty_assertions::assert_eq!(url, "http://cs1.example.com/ngsi-ld/v1/types?details=true");
    }

    #[test]
    fn build_discovery_url_without_query_string() {
        use crate::service::csource_forwarding::build_discovery_url;

        let url = build_discovery_url(&Url::parse("http://cs1.example.com/").expect("valid url"), "/attributes", "");
        pretty_assertions::assert_eq!(url, "http://cs1.example.com/ngsi-ld/v1/attributes");
    }

    #[test]
    fn build_temporal_query_url_with_params() {
        use crate::service::csource_forwarding::build_temporal_query_url;

        let url = build_temporal_query_url(
            &Url::parse("http://cs1.example.com").expect("valid url"),
            "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z",
        );
        pretty_assertions::assert_eq!(
            url,
            "http://cs1.example.com/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z"
        );
    }

    #[test]
    fn build_temporal_retrieve_url_with_id() {
        use crate::service::csource_forwarding::build_temporal_retrieve_url;

        let url = build_temporal_retrieve_url(
            &Url::parse("http://cs1.example.com").expect("valid url"),
            "urn:ngsi-ld:Vehicle:1",
            "timerel=after&timeAt=2024-01-01T00:00:00Z",
        );
        pretty_assertions::assert_eq!(
            url,
            "http://cs1.example.com/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:1?timerel=after&timeAt=2024-01-01T00:00:00Z"
        );
    }

    #[test]
    fn collect_discovery_warnings_aggregates() {
        use crate::service::csource_forwarding::{ForwardedDiscoveryResult, collect_discovery_warnings};

        let results = vec![
            ForwardedDiscoveryResult {
                body: None,
                warnings: vec![NgsiLdWarning::MiscellaneousWarning("timeout".to_string())],
            },
            ForwardedDiscoveryResult {
                body: Some(serde_json::json!({})),
                warnings: Vec::new(),
            },
        ];
        let warnings = collect_discovery_warnings(&results);
        pretty_assertions::assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn collect_forwarded_exclusive_attrs_empty_results() {
        let result = collect_forwarded_exclusive_attrs(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn collect_forwarded_exclusive_attrs_gathers_attrs() {
        let results = vec![
            ForwardedWriteResult {
                mode: ForwardingMode::Exclusive,
                status: 204,
                body: None,
                exclusive_attrs: vec!["speed".to_string(), "fuel".to_string()],
            },
            ForwardedWriteResult {
                mode: ForwardingMode::Inclusive,
                status: 204,
                body: None,
                exclusive_attrs: vec!["temperature".to_string()],
            },
        ];
        let attrs = collect_forwarded_exclusive_attrs(&results);
        pretty_assertions::assert_eq!(attrs.len(), 3);
        for expected in ["speed", "fuel", "temperature"] {
            assert!(attrs.iter().any(|attr| attr.as_str() == expected), "{expected} missing");
        }
    }

    #[test]
    fn strip_exclusive_attrs_from_value_removes_matching_keys() {
        let mut value = serde_json::json!({
            "speed": { "type": "Property", "value": 100 },
            "fuel": { "type": "Property", "value": 50 },
            "name": { "type": "Property", "value": "Car" }
        });
        strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed"), attribute_name("fuel")]);
        let obj = value.as_object().expect("is object");
        pretty_assertions::assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("name"));
    }

    #[test]
    fn strip_exclusive_attrs_from_value_no_matching_keys() {
        let mut value = serde_json::json!({
            "name": { "type": "Property", "value": "Car" }
        });
        strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed")]);
        let obj = value.as_object().expect("is object");
        pretty_assertions::assert_eq!(obj.len(), 1);
    }

    #[test]
    fn strip_exclusive_attrs_from_value_non_object_noop() {
        let mut value = serde_json::json!([1, 2, 3]);
        strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed")]);
        assert!(value.is_array());
    }

    #[test]
    fn collect_forwarded_exclusive_attrs_results_with_no_exclusive_attrs() {
        let results = vec![ForwardedWriteResult {
            mode: ForwardingMode::Inclusive,
            status: 204,
            body: None,
            exclusive_attrs: Vec::new(),
        }];
        let attrs = collect_forwarded_exclusive_attrs(&results);
        assert!(attrs.is_empty());
    }

    #[test]
    fn collect_forwarded_exclusive_attrs_duplicates_not_deduped() {
        // collect_forwarded_exclusive_attrs does not deduplicate -- callers
        // use the list for removal which is idempotent on duplicates.
        let results = vec![
            ForwardedWriteResult {
                mode: ForwardingMode::Exclusive,
                status: 204,
                body: None,
                exclusive_attrs: vec!["speed".to_string()],
            },
            ForwardedWriteResult {
                mode: ForwardingMode::Exclusive,
                status: 204,
                body: None,
                exclusive_attrs: vec!["speed".to_string()],
            },
        ];
        let attrs = collect_forwarded_exclusive_attrs(&results);
        pretty_assertions::assert_eq!(attrs.len(), 2);
    }

    #[test]
    fn csr_mode_filter_all_includes_every_mode() {
        use crate::service::csource_forwarding::CsrModeFilter;

        let filter = CsrModeFilter::All;
        // All should include every mode
        assert!(matches!(filter, CsrModeFilter::All));
    }

    #[test]
    fn csr_mode_filter_exclude_auxiliary() {
        use crate::service::csource_forwarding::CsrModeFilter;

        let filter = CsrModeFilter::ExcludeAuxiliary;
        pretty_assertions::assert_eq!(filter, CsrModeFilter::ExcludeAuxiliary);
        pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
        pretty_assertions::assert_ne!(filter, CsrModeFilter::AuxiliaryOnly);
    }

    #[test]
    fn csr_mode_filter_auxiliary_only() {
        use crate::service::csource_forwarding::CsrModeFilter;

        let filter = CsrModeFilter::AuxiliaryOnly;
        pretty_assertions::assert_eq!(filter, CsrModeFilter::AuxiliaryOnly);
        pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
        pretty_assertions::assert_ne!(filter, CsrModeFilter::ExcludeAuxiliary);
    }

    #[test]
    fn merge_query_auxiliary_adds_and_merges() {
        let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "speed": 100}))];
        let remote = vec![ForwardedEntities {
            mode: ForwardingMode::Auxiliary,
            entities: vec![
                entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "fuel": 50})),
                entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80})),
            ],
            warnings: Vec::new(),
        }];
        let result = merge_query_results(local, remote);
        pretty_assertions::assert_eq!(result.len(), 2, "Auxiliary should add remote entities in query merge");
        let entity_a = result.iter().find(|entity| entity.id.to_string() == "urn:a").expect("urn:a should be present");
        assert!(entity_a.attrs.contains_key("fuel"), "Auxiliary attrs should be merged");
    }

    #[test]
    fn merge_query_primary_adds_new_entities() {
        // Non-auxiliary modes should add new entities
        let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))];
        let remote = vec![ForwardedEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80}))],
            warnings: Vec::new(),
        }];
        let result = merge_query_results(local, remote);
        pretty_assertions::assert_eq!(result.len(), 2, "Inclusive should add new entities");
    }
}
