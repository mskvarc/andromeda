use crate::{
    api::header::{jsonld_context_link, link_header_value},
    domain::registration::{ContextSourceRegistration, CsrOperation, ForwardingMode},
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    repository::context_source_registration::ContextSourceRegistrationRepository,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use tracing::{debug, warn};

/// Service for forwarding NGSI-LD operations to registered context sources.
pub struct ContextSourceForwardingService {
    http_client: reqwest::Client,
    csr_repo: ContextSourceRegistrationRepository,
    engine: JsonLdEngine,
    /// Context Source Identity (hostAlias) for Via header (§6.3.18).
    source_identity: String,
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
}

impl ForwardingOperation {
    /// Returns the operation strings that match this forwarding operation.
    /// Per NGSI-LD spec 6.3.5: if `operations` is not specified on a CSR, it supports all operations.
    fn matches(&self, csr: &ContextSourceRegistration) -> bool {
        let ops = match &csr.operations {
            Some(ops) => ops,
            None => return true, // No operations specified = supports all
        };

        let mode = csr.mode.unwrap_or_default();

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
        }
    }
}

/// IANA warning codes for distributed operations (§6.3.17).
#[derive(Debug, Clone)]
pub enum NgsiLdWarning {
    /// 199: No response within timeout or registration loop detected.
    MiscellaneousWarning(String),
    /// 299: Error response from registration endpoint (e.g., 403).
    MiscellaneousPersistentWarning(String),
}

impl NgsiLdWarning {
    /// Format as NGSILD-Warning header value per IANA Warning specification.
    pub fn header_value(&self) -> String {
        match self {
            NgsiLdWarning::MiscellaneousWarning(msg) => format!("199 - \"{}\"", msg),
            NgsiLdWarning::MiscellaneousPersistentWarning(msg) => format!("299 - \"{}\"", msg),
        }
    }
}

/// Result from a single context source forwarding attempt.
pub struct ForwardedEntities {
    /// The forwarding mode of the matched context source registration.
    pub mode: ForwardingMode,
    /// Entities returned by the context source.
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
    pub incoming_via: Option<&'a str>,
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
    pub incoming_via: Option<&'a str>,
}

/// Marker indicating an exclusive CSR matched the entity but not the requested operation.
/// Used to detect when exclusive data exists but can't be forwarded (e.g. no redirectionOps).
pub struct UnforwardableExclusiveCsr;

impl ContextSourceForwardingService {
    /// Create a new context source forwarding service with the given source identity for Via headers.
    pub fn new(pool: &PgPool, engine: JsonLdEngine, source_identity: String) -> Result<ContextSourceForwardingService, AndromedaError> {
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
        let csr_repo = ContextSourceRegistrationRepository::new(pool);
        Ok(ContextSourceForwardingService {
            http_client,
            csr_repo,
            engine,
            source_identity,
        })
    }

    /// Forward an entity query to matching context sources.
    ///
    /// `entity_type_compact` is the compact entity type from the query (e.g., "Vehicle").
    /// It will be expanded using the context for CSR matching.
    /// `entity_ids` are optional entity IDs to filter by (for query-by-id forwarding).
    pub async fn forward_query(
        &self,
        entity_type_compact: Option<&str>,
        entity_ids: Option<&[String]>,
        context_url: Option<&str>,
        context: &ContextSpec,
        operation: ForwardingOperation,
        incoming_via: Option<&str>,
    ) -> Result<Vec<ForwardedEntities>, AndromedaError> {
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
        let matching_csrs: Vec<_> = csrs.into_iter().filter(|csr| operation.matches(csr)).collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        // Build a minimal forwarding query string — only type and id params
        // Context sources return all attributes; attribute filtering is done locally after merge
        let forwarding_qs = build_forwarding_query_string(entity_type_compact, entity_ids);

        let mut results = Vec::new();
        for csr in matching_csrs {
            let url = build_query_url(&csr.endpoint, &forwarding_qs);
            debug!("Forwarding entity query to context source {}: {}", csr.id, url);

            match self.forward_get(&url, context_url, csr.tenant.as_deref(), incoming_via).await {
                Ok(body) => {
                    let entities = match body {
                        Value::Array(arr) => arr,
                        obj @ Value::Object(_) => vec![obj], // Single entity response
                        _other => {
                            warn!("Context source {} returned unexpected response type", csr.id);
                            continue;
                        }
                    };
                    let mode = csr.mode.unwrap_or_default();
                    results.push(ForwardedEntities {
                        mode,
                        entities,
                        warnings: Vec::new(),
                    });
                }
                Err(e) => {
                    warn!("Failed to forward query to context source {}: {}", csr.id, e);
                    let mode = csr.mode.unwrap_or_default();
                    results.push(ForwardedEntities {
                        mode,
                        entities: Vec::new(),
                        warnings: vec![NgsiLdWarning::MiscellaneousWarning(format!(
                            "Context source {} did not respond in time",
                            csr.id
                        ))],
                    });
                }
            }
        }

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
        incoming_via: Option<&str>,
    ) -> Result<Vec<ForwardedEntities>, AndromedaError> {
        if self.is_loop_detected(incoming_via) {
            debug!("Loop detected in Via header chain, skipping retrieve forwarding");
            return Ok(Vec::new());
        }

        let csrs = self.csr_repo.find_matching(None, Some(entity_id)).await?;
        let matching_csrs: Vec<_> = csrs.into_iter().filter(|csr| operation.matches(csr)).collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for csr in matching_csrs {
            let url = build_retrieve_url(&csr.endpoint, entity_id, query_string);
            debug!("Forwarding entity retrieve to context source {}: {}", csr.id, url);

            match self.forward_get(&url, context_url, csr.tenant.as_deref(), incoming_via).await {
                Ok(body) => {
                    // Single entity response — wrap in array for uniform handling
                    if body.is_object() {
                        let mode = csr.mode.unwrap_or_default();
                        results.push(ForwardedEntities {
                            mode,
                            entities: vec![body],
                            warnings: Vec::new(),
                        });
                    }
                    // 404 responses will have been caught as non-success status
                }
                Err(e) => {
                    debug!("Forward retrieve to context source {} failed (may be 404): {}", csr.id, e);
                    let mode = csr.mode.unwrap_or_default();
                    results.push(ForwardedEntities {
                        mode,
                        entities: Vec::new(),
                        warnings: vec![NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                            "Context source {} returned error: {}",
                            csr.id, e
                        ))],
                    });
                }
            }
        }

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

        // Separate CSRs into those that match the operation and exclusive ones that don't
        let mut matching_csrs = Vec::new();
        let mut unforwardable = Vec::new();
        for csr in csrs {
            if request.operation.matches(&csr) {
                matching_csrs.push(csr);
            } else if csr.mode == Some(ForwardingMode::Exclusive) {
                // Per spec 4.3.6.3: exclusive CSR exists but doesn't support this operation
                unforwardable.push(UnforwardableExclusiveCsr);
            }
        }

        if matching_csrs.is_empty() {
            return Ok((Vec::new(), unforwardable));
        }

        let mut results = Vec::new();
        for csr in matching_csrs {
            let url = if request.method == reqwest::Method::POST && request.path_suffix.is_none() {
                // Entity create: POST /entities (no entity ID in path)
                build_create_url(&csr.endpoint)
            } else {
                let id = request.entity_id.unwrap_or("");
                let encoded_id = percent_encoding::utf8_percent_encode(id, PATH_ENCODE_SET).to_string();
                let base = csr.endpoint.trim_end_matches('/');
                let suffix = request.path_suffix.unwrap_or("");
                match request.query_string {
                    Some(qs) if !qs.is_empty() => format!("{base}/ngsi-ld/v1/entities/{encoded_id}{suffix}?{qs}"),
                    _ => format!("{base}/ngsi-ld/v1/entities/{encoded_id}{suffix}"),
                }
            };
            debug!("Forwarding {} to context source {}: {}", request.method, csr.id, url);

            let exclusive_attrs = if csr.mode == Some(ForwardingMode::Exclusive) {
                collect_exclusive_attrs(&csr)
            } else {
                Vec::new()
            };

            match self
                .forward_write(
                    &url,
                    request.method.clone(), // clone: reqwest::Method is reused across loop iterations
                    request.body,
                    request.context_url,
                    csr.tenant.as_deref(),
                    request.incoming_via,
                )
                .await
            {
                Ok((status, response_body)) => {
                    let mode = csr.mode.unwrap_or_default();
                    results.push(ForwardedWriteResult {
                        mode,
                        status,
                        body: response_body,
                        exclusive_attrs,
                    });
                }
                Err(e) => {
                    warn!("Failed to forward {} to context source {}: {}", request.method, csr.id, e);
                }
            }
        }

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
        let matching_csrs: Vec<_> = csrs.into_iter().filter(|csr| request.operation.matches(csr)).collect();

        if matching_csrs.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for csr in matching_csrs {
            let base = csr.endpoint.trim_end_matches('/');
            let url = match request.query_string {
                Some(qs) if !qs.is_empty() => format!("{base}/ngsi-ld/v1{}?{qs}", request.batch_path),
                _ => format!("{base}/ngsi-ld/v1{}", request.batch_path),
            };
            debug!("Forwarding batch {} to context source {}: {}", request.batch_path, csr.id, url);

            let exclusive_attrs = if csr.mode == Some(ForwardingMode::Exclusive) {
                collect_exclusive_attrs(&csr)
            } else {
                Vec::new()
            };

            match self
                .forward_write(
                    &url,
                    reqwest::Method::POST,
                    Some(request.body),
                    request.context_url,
                    csr.tenant.as_deref(),
                    request.incoming_via,
                )
                .await
            {
                Ok((status, response_body)) => {
                    let mode = csr.mode.unwrap_or_default();
                    results.push(ForwardedWriteResult {
                        mode,
                        status,
                        body: response_body,
                        exclusive_attrs,
                    });
                }
                Err(e) => {
                    warn!("Failed to forward batch to context source {}: {}", csr.id, e);
                }
            }
        }

        Ok(results)
    }

    /// Make a write request (POST/PUT/PATCH/DELETE) to a context source endpoint.
    async fn forward_write(
        &self,
        url: &str,
        method: reqwest::Method,
        body: Option<&[u8]>,
        context_url: Option<&str>,
        tenant: Option<&str>,
        incoming_via: Option<&str>,
    ) -> Result<(u16, Option<Value>), AndromedaError> {
        debug!("CS forwarding {} {}", method, url);

        let mut request = self.build_request(method, url, context_url, tenant, incoming_via);

        if let Some(body_bytes) = body {
            request = request.header("Content-Type", "application/ld+json").body(body_bytes.to_vec());
        }

        let (status, bytes) = self.send_request(request).await?;

        let response_body = if bytes.is_empty() {
            None
        } else {
            let text = String::from_utf8_lossy(&bytes);
            serde_json::from_str(&text).ok()
        };

        Ok((status.as_u16(), response_body))
    }

    /// Make a GET request to a context source endpoint.
    async fn forward_get(&self, url: &str, context_url: Option<&str>, tenant: Option<&str>, incoming_via: Option<&str>) -> Result<Value, AndromedaError> {
        debug!("CS forwarding GET {}", url);

        let request = self
            .build_request(reqwest::Method::GET, url, context_url, tenant, incoming_via)
            .header("Accept", "application/json");

        let (status, bytes) = self.send_request(request).await?;

        if !status.is_success() {
            return Err(AndromedaError::internal(format!("Context source returned HTTP {}", status)));
        }

        let text = String::from_utf8_lossy(&bytes);

        serde_json::from_str(&text).map_err(|e| {
            warn!("Failed to parse CS response as JSON: {}. Body: {}", e, &text[..text.len().min(200)]);
            AndromedaError::internal(format!("Failed to parse context source response as JSON: {}", e))
        })
    }

    /// Build a request with shared headers (Connection: close, Link context, Via).
    fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
        context_url: Option<&str>,
        tenant: Option<&str>,
        incoming_via: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut request = self.http_client.request(method, url).header("Connection", "close");

        if let Some(ctx) = context_url {
            let link = jsonld_context_link(ctx);
            if let Some(value) = link_header_value(&link) {
                request = request.header("Link", value);
            }
        }

        if let Some(tenant) = tenant {
            request = request.header("NGSILD-Tenant", tenant);
        }

        // Append our identity to the Via header chain (§6.3.18)
        let our_via = format!("1.1 {}", self.source_identity);
        let via_value = match incoming_via {
            Some(existing) => format!("{}, {}", existing, our_via),
            None => our_via,
        };
        request = request.header("Via", via_value);

        request
    }

    /// Check whether forwarding would create a loop based on the Via header chain.
    pub fn is_loop_detected(&self, incoming_via: Option<&str>) -> bool {
        is_via_loop(incoming_via, &self.source_identity)
    }

    /// Send a request and return the status and raw bytes.
    async fn send_request(&self, request: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, Vec<u8>), AndromedaError> {
        let response = request.send().await.map_err(|e| AndromedaError::map_reqwest(e, "context_source_forward"))?;

        let status = response.status();

        let bytes = response.bytes().await.map_err(|e| AndromedaError::map_reqwest(e, "context_source_read_body"))?;

        Ok((status, bytes.to_vec()))
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
fn build_create_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    format!("{}/ngsi-ld/v1/entities", base)
}

/// Build a forwarding URL for entity queries.
/// The endpoint may already contain a path (e.g., for redirect mode).
fn build_query_url(endpoint: &str, query_string: &str) -> String {
    let base = endpoint.trim_end_matches('/');
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
fn build_retrieve_url(endpoint: &str, entity_id: &str, query_string: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    let encoded_id = percent_encoding::utf8_percent_encode(entity_id, PATH_ENCODE_SET).to_string();
    if query_string.is_empty() {
        format!("{}/ngsi-ld/v1/entities/{}", base, encoded_id)
    } else {
        format!("{}/ngsi-ld/v1/entities/{}?{}", base, encoded_id, query_string)
    }
}

/// Collect property and relationship names from an exclusive CSR's information blocks.
/// Returns both the expanded IRI and the compacted (short) name for each attribute,
/// so callers can match against either form.
fn collect_exclusive_attrs(csr: &ContextSourceRegistration) -> Vec<String> {
    let mut attrs = Vec::new();
    for info in &csr.information {
        if let Some(ref props) = info.property_names {
            for prop in props {
                attrs.push(prop.clone()); // clone: owned value for collected attrs Vec
                // Also add compacted form (last path/fragment segment) for matching
                // against deserialized EntityPatch attr keys which are compacted
                if let Some(short) = compact_iri(prop)
                    && short != *prop
                {
                    attrs.push(short);
                }
            }
        }
        if let Some(ref rels) = info.relationship_names {
            for rel in rels {
                attrs.push(rel.clone()); // clone: owned value for collected attrs Vec
                if let Some(short) = compact_iri(rel)
                    && short != *rel
                {
                    attrs.push(short);
                }
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

/// Merge local and remote entity query results.
///
/// Per NGSI-LD spec:
/// - **inclusive** (default): Include both local and remote entities. If same ID exists, merge attributes.
/// - **redirect**: Same as inclusive for query results.
/// - **auxiliary**: Only supplement existing local entities with remote attributes. Don't add new entities.
/// - **exclusive**: Only remote entities (local entities were created with `local=true` and stay local-only).
pub fn merge_query_results(local_entities: Vec<Value>, remote_results: Vec<ForwardedEntities>) -> Vec<Value> {
    if remote_results.is_empty() {
        return local_entities;
    }

    // Build a map of entity ID -> entity value for dedup/merge
    let mut entity_map: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();

    // Add local entities first
    for entity in local_entities {
        if let Some(id) = entity.get("id").and_then(|v| v.as_str()) {
            entity_map.insert(id.to_string(), entity);
        }
    }

    // Process remote results based on mode
    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary => {
                // Only merge attributes into entities that already exist locally
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str())
                        && let Some(local_entity) = entity_map.get_mut(remote_id)
                    {
                        merge_entity_attributes(local_entity, &remote_entity);
                    }
                    // Don't add new entities in auxiliary mode
                }
            }
            ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                // inclusive, redirect, exclusive: add/merge remote entities
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()) {
                        if let Some(local_entity) = entity_map.get_mut(remote_id) {
                            merge_entity_attributes(local_entity, &remote_entity);
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

/// Merge remote entity's attributes into a local entity.
/// Remote attributes that don't exist locally are added.
/// Local attributes take precedence (are not overwritten).
fn merge_entity_attributes(local: &mut Value, remote: &Value) {
    if let (Some(local_obj), Some(remote_obj)) = (local.as_object_mut(), remote.as_object()) {
        for (key, value) in remote_obj {
            // Skip metadata fields
            if key == "id" || key == "type" || key == "@context" {
                continue;
            }
            // Only add attributes that don't exist locally
            if !local_obj.contains_key(key) {
                local_obj.insert(key.clone(), value.clone()); // clone: owned key-value pair for merging remote attributes into local entity
            }
        }
    }
}

/// Merge remote entity results for a single entity retrieval.
/// Returns the merged entity or None if no entity found from any source.
pub fn merge_retrieve_results(local_entity: Option<Value>, remote_results: Vec<ForwardedEntities>) -> Option<Value> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let is_auxiliary = forwarded.mode == ForwardingMode::Auxiliary;
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => {
                    merge_entity_attributes(existing, &remote_entity);
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

/// Collect all warnings from forwarded results.
pub fn collect_warnings(results: &[ForwardedEntities]) -> Vec<String> {
    results.iter().flat_map(|r| r.warnings.iter().map(|w| w.header_value())).collect()
}

/// Check if the incoming Via header chain already contains our identity (loop detection).
pub fn is_via_loop(incoming_via: Option<&str>, source_identity: &str) -> bool {
    match incoming_via {
        Some(via) => via.contains(source_identity),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::registration::ForwardingMode,
        service::context_source_forwarding::{ForwardedEntities, NgsiLdWarning, collect_warnings, is_via_loop, merge_query_results},
    };

    const ID: &str = "urn:ngsi-ld:ContextSourceIdentity:broker1";

    #[test]
    fn via_loop_no_header() {
        assert!(!is_via_loop(None, ID));
    }

    #[test]
    fn via_loop_not_in_chain() {
        assert!(!is_via_loop(Some("1.1 urn:ngsi-ld:ContextSourceIdentity:broker2"), ID));
    }

    #[test]
    fn via_loop_detected_single() {
        assert!(is_via_loop(Some("1.1 urn:ngsi-ld:ContextSourceIdentity:broker1"), ID));
    }

    #[test]
    fn via_loop_detected_multi_hop() {
        assert!(is_via_loop(
            Some("1.1 urn:ngsi-ld:ContextSourceIdentity:broker2, 1.1 urn:ngsi-ld:ContextSourceIdentity:broker1"),
            ID,
        ));
    }

    #[test]
    fn warning_header_format_miscellaneous() {
        let w = NgsiLdWarning::MiscellaneousWarning("timeout".to_string());
        assert_eq!(w.header_value(), "199 - \"timeout\"");
    }

    #[test]
    fn warning_header_format_persistent() {
        let w = NgsiLdWarning::MiscellaneousPersistentWarning("403 Forbidden".to_string());
        assert_eq!(w.header_value(), "299 - \"403 Forbidden\"");
    }

    #[test]
    fn collect_warnings_from_results() {
        let results = vec![
            ForwardedEntities {
                mode: ForwardingMode::Inclusive,
                entities: Vec::new(),
                warnings: vec![NgsiLdWarning::MiscellaneousWarning("timeout".to_string())],
            },
            ForwardedEntities {
                mode: ForwardingMode::Inclusive,
                entities: vec![serde_json::json!({"id": "urn:a"})],
                warnings: Vec::new(),
            },
        ];
        let warnings = collect_warnings(&results);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("199"));
    }

    #[test]
    fn merge_query_local_only() {
        let local = vec![serde_json::json!({"id": "urn:a", "type": "T", "p1": 1})];
        let result = merge_query_results(local, Vec::new());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn merge_query_adds_remote_entities() {
        let local = vec![serde_json::json!({"id": "urn:a", "type": "T"})];
        let remote = vec![ForwardedEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![serde_json::json!({"id": "urn:b", "type": "T"})],
            warnings: Vec::new(),
        }];
        let result = merge_query_results(local, remote);
        assert_eq!(result.len(), 2);
    }
}
