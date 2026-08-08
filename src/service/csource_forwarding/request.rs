//! Typed inputs for context-source forwarding operations.

use crate::service::csource_forwarding::operation::ForwardingOperation;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_http_headers::via::Via;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::filter::expression::Expr;

/// Parameters for forwarding a single-entity provision operation.
pub struct ForwardProvisionRequest<'a> {
    /// Entity ID to target, if applicable.
    pub entity_id: Option<&'a str>,
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Attribute name used to filter registration information entries.
    pub attribute_name: Option<&'a AttributeName>,
    /// HTTP method for the forwarded request.
    pub method: reqwest::Method,
    /// Optional request body bytes.
    pub body: Option<&'a [u8]>,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context used to expand matching terms.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Path suffix below the entity resource.
    pub path_suffix: Option<&'a str>,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// Incoming Via header for loop detection.
    pub incoming_via: Option<&'a Via>,
}

/// Parameters for forwarding a batch provision operation.
pub struct ForwardBatchProvisionRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Batch request body.
    pub body: &'a [u8],
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context used to expand matching terms.
    pub context: &'a ContextSpec,
    /// NGSI-LD batch operation being forwarded.
    pub operation: ForwardingOperation,
    /// Batch endpoint path.
    pub batch_path: &'a str,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// Incoming Via header for loop detection.
    pub incoming_via: Option<&'a Via>,
}

/// Parameters for forwarding a temporal entity query.
pub struct ForwardTemporalQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Optional entity identifiers.
    pub entity_ids: Option<&'a [Uri]>,
    /// Complete temporal query string.
    pub query_string: &'a str,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context used to expand matching terms.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Incoming Via header for loop detection.
    pub incoming_via: Option<&'a Via>,
}

/// Parameters for forwarding an entity query.
pub struct ForwardQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a str>,
    /// Optional entity identifiers.
    pub entity_ids: Option<&'a [Uri]>,
    /// JSON-LD context URL for the Link header.
    pub context_url: Option<&'a str>,
    /// JSON-LD context used to expand matching terms.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Incoming Via header for loop detection.
    pub incoming_via: Option<&'a Via>,
    /// Optional context-source filter expression.
    pub csf_expr: Option<&'a Expr>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
}

/// Registration modes included in a forwarding phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsrModeFilter {
    /// Include all registration modes.
    All,
    /// Exclude auxiliary registrations.
    ExcludeAuxiliary,
    /// Include only auxiliary registrations.
    AuxiliaryOnly,
}
