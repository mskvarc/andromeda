//! Typed inputs for context-source federation operations.

use crate::operation::ForwardingOperation;
use andromeda_http_headers::via::Via;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_ngsi_ld_query::{filter::expression::Expr, type_selection::grammar::TypeExprRaw};
use http::HeaderMap;

/// The headers of the request that triggered a forwarding, read for the
/// `contextSourceInfo` pairs whose value is `urn:ngsi-ld:request` (clause
/// 6.3.19) and for the `Via` chain to extend (clause 6.3.18).
#[derive(Debug, Clone, Copy, Default)]
pub struct TriggeringRequest<'a> {
    /// The incoming `Via` header for loop detection; absent when this broker
    /// originates the operation.
    pub via: Option<&'a Via>,
    /// The full incoming header map, for pass-through values.
    pub headers: Option<&'a HeaderMap>,
}

/// Parameters for forwarding a single-entity provision operation.
pub struct ForwardProvisionRequest<'a> {
    /// Entity ID to target, if applicable.
    pub entity_id: &'a Uri,
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a EntityTypeName>,
    /// Attribute name used to filter registration information entries.
    pub attribute_name: Option<&'a AttributeName>,
    /// HTTP method for the forwarded request.
    pub method: reqwest::Method,
    /// Optional request body bytes, without an inline `@context`.
    pub body: Option<&'a [u8]>,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// Path suffix below the entity resource.
    pub path_suffix: Option<&'a str>,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
}

/// Parameters for forwarding a batch provision operation.
pub struct ForwardBatchProvisionRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a EntityTypeName>,
    /// Batch request body, without an inline `@context`.
    pub body: &'a [u8],
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD batch operation being forwarded.
    pub operation: ForwardingOperation,
    /// Batch endpoint path below the API base.
    pub batch_path: &'a str,
    /// Query string appended to the forwarded URL.
    pub query_string: Option<&'a str>,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
}

/// Parameters for forwarding a temporal entity query.
pub struct ForwardTemporalQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a TypeExprRaw>,
    /// Optional entity identifiers.
    pub entity_ids: Option<&'a [Uri]>,
    /// Complete temporal query string.
    pub query_string: &'a str,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The live EntityMap the request referenced, if any (clause 5.5.14): only
    /// its sources are contacted, each told its own linked map, and clause
    /// 5.12 matching is skipped.
    pub entity_map: Option<&'a EntityMap>,
}

/// Parameters for forwarding an entity query.
pub struct ForwardQueryRequest<'a> {
    /// Compact entity type for CSR matching.
    pub entity_type_compact: Option<&'a TypeExprRaw>,
    /// Optional entity identifiers.
    pub entity_ids: Option<&'a [Uri]>,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// Optional context-source filter expression.
    pub csf_expr: Option<&'a Expr>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The live EntityMap the request referenced, if any (clause 5.5.14): only
    /// its sources are contacted, each told its own linked map, and clause
    /// 5.12 matching is skipped.
    pub entity_map: Option<&'a EntityMap>,
}

/// Parameters for forwarding a single-entity retrieval, current or temporal.
pub struct ForwardRetrieveRequest<'a> {
    /// The entity to retrieve.
    pub entity_id: &'a Uri,
    /// A registration that is not contacted: the sender of a notification
    /// being re-assembled already supplied its part (clause 5.8.6).
    pub exclude_registration: Option<&'a Uri>,
    /// The query string of the triggering request.
    pub query_string: &'a str,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The live EntityMap the request referenced, if any (clause 5.5.14): only
    /// its sources are contacted, each told its own linked map, and clause
    /// 5.12 matching is skipped.
    pub entity_map: Option<&'a EntityMap>,
}

/// Parameters for forwarding a type or attribute discovery query.
pub struct ForwardDiscoveryRequest<'a> {
    /// The discovery resource path below the API base, such as `/types`.
    pub path: &'a str,
    /// The query string of the triggering request.
    pub query_string: &'a str,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// The discovery operation being forwarded (clauses 5.7.5 to 5.7.10).
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
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
