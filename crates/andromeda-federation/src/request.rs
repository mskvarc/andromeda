//! Typed inputs for context-source federation operations.

use crate::{
    config::SplitEntities,
    forwarded_query::ClientQuery,
    operation::{ForwardingOperation, ProvisionResource},
};
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_http_headers::via::Via;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_ngsi_ld_query::filter::expression::Expr;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use http::HeaderMap;

/// An explicit page to request independently from every matching context source.
///
/// Ordinary distributed reads leave this absent because the broker obtains a
/// merge window and applies the client's page after aggregation. Durable range
/// consumers use it to walk every source without repeatedly reading that first
/// merge window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePage {
    /// Maximum number of results requested from each source.
    pub limit: u32,
    /// Number of source-local results skipped before this page.
    pub offset: u32,
}

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

/// The EntityMap a read runs under (clause 5.5.14).
#[derive(Debug, Clone, Copy)]
pub enum EntityMapInUse<'a> {
    /// No map: the registrations are matched by clause 5.12 and no location
    /// is forwarded.
    None,
    /// A live referenced map: only its sources are contacted, each told the
    /// map its source used before, or this map when no link is recorded yet.
    Referenced(&'a EntityMap),
    /// The map being minted for this request: the registrations are matched
    /// by clause 5.12 and every source is told the map's location, so it can
    /// join the unitary sequence from its first request.
    Minting(&'a EntityMap),
}

impl<'a> From<&'a EntityMapUse> for EntityMapInUse<'a> {
    fn from(map_use: &'a EntityMapUse) -> EntityMapInUse<'a> {
        match map_use {
            EntityMapUse::Reuse(map) => EntityMapInUse::Referenced(map),
            EntityMapUse::Mint(map) => EntityMapInUse::Minting(map),
            EntityMapUse::None => EntityMapInUse::None,
        }
    }
}

impl EntityMapInUse<'_> {
    /// The live map that fixes the registrations, if one is referenced.
    pub const fn referenced(&self) -> Option<&EntityMap> {
        match self {
            EntityMapInUse::Referenced(map) => Some(map),
            EntityMapInUse::Minting(_) | EntityMapInUse::None => None,
        }
    }

    /// The `NGSILD-EntityMap` a forwarded request to `registration` carries.
    pub fn location_for(&self, registration: &Uri) -> Option<&Uri> {
        match self {
            EntityMapInUse::Referenced(map) => Some(map.linked_map_for(registration).unwrap_or(&map.id)),
            EntityMapInUse::Minting(map) => Some(&map.id),
            EntityMapInUse::None => None,
        }
    }
}

/// Parameters for forwarding a single-entity provision operation.
pub struct ForwardProvisionRequest<'a> {
    /// The resource the operation addresses: the entity or its temporal
    /// evolution.
    pub resource: ProvisionResource,
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

/// Parameters for forwarding a temporal entity query.
pub struct ForwardTemporalQueryRequest<'a> {
    /// The client's temporal query, narrowed per registration before it is
    /// forwarded (clause 4.3.6.1) and the source of the type and identifiers
    /// the registrations are matched against.
    pub client_query: &'a TemporalQueryParameters,
    /// Whether the attribute-reading filters stay with the broker (clause 5.7.4.4).
    pub split: SplitEntities,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// The expanded context source filter selecting the registrations to
    /// contact (clause 5.7.4.3), if the client sent one.
    pub csf_expr: Option<&'a Expr>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The EntityMap the request runs under (clause 5.5.14).
    pub entity_map: EntityMapInUse<'a>,
    /// Explicit per-source range, or `None` for the ordinary post-merge page policy.
    pub source_page: Option<SourcePage>,
}

/// Parameters for forwarding an entity query.
pub struct ForwardQueryRequest<'a> {
    /// The client's query, narrowed per registration before it is forwarded
    /// (clause 4.3.6.1) and the source of the type and identifiers the
    /// registrations are matched against.
    pub client_query: ClientQuery<'a>,
    /// Whether the attribute-reading filters stay with the broker (clause 5.7.2.4).
    pub split: SplitEntities,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// The expanded context source filter selecting the registrations to
    /// contact (Table 6.4.3.2-1 `csf`), if the client sent one.
    pub csf_expr: Option<&'a Expr>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The EntityMap the request runs under (clause 5.5.14).
    pub entity_map: EntityMapInUse<'a>,
    /// Explicit per-source range, or `None` for the ordinary post-merge page policy.
    pub source_page: Option<SourcePage>,
}

/// Parameters for forwarding a single-entity retrieval, current or temporal.
pub struct ForwardRetrieveRequest<'a> {
    /// The entity to retrieve.
    pub entity_id: &'a Uri,
    /// A registration that is not contacted: the sender of a notification
    /// being re-assembled already supplied its part (clause 5.8.6).
    pub exclude_registration: Option<&'a Uri>,
    /// The client's retrieval parameters, narrowed per registration before
    /// they are forwarded (clause 4.3.6.1). Clauses 5.7.1.4 and 5.7.3.4 know
    /// no split entities, so every parameter that is not narrowed travels.
    pub client_query: ClientQuery<'a>,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// NGSI-LD operation being forwarded.
    pub operation: ForwardingOperation,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
    /// Registration modes included in this forwarding phase.
    pub mode_filter: CsrModeFilter,
    /// The EntityMap the request runs under (clause 5.5.14).
    pub entity_map: EntityMapInUse<'a>,
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
