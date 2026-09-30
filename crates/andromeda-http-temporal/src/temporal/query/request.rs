//! The query-string binding of "Query Temporal Evolution of Entities"
//! (clause 6.18.3.2), and the request both bindings hand the operation.
//!
//! Clause 6.24.1 says of the POST binding of clause 6.24.3.1 that "the
//! behaviour of this clause mirrors the one in clause 6.18.3.2", so once a
//! binding has decoded the query the two are one [`TemporalQueryRequest`], which
//! [`crate::temporal::query::run`] runs.

use crate::{
    grouping_gate::GroupingGate,
    state::TemporalRouteServices,
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        Response,
        TemporalQueryParameters,
        TenantState,
        query::run,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_extraction::{
    admitted_temporal_query::AdmittedTemporalQuery,
    api_root::ApiRoot,
    context_target,
    response_context_request::ResponseContextRequest,
};
use andromeda_http_response::pagination::PagePointers;
use axum::http::{HeaderMap, Uri};
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One "Query Temporal Evolution of Entities" as its binding decoded it.
///
/// Everything here is what the operation needs and nothing is specific to the
/// binding that supplied it: the query string binding of clause 6.18.3.2 fills
/// [`TemporalQueryRequest::params`] from the URL, the POST binding of clause
/// 6.24.3.1 fills it from the clause 5.2.23 body, and from there on the two are
/// the same request.
#[derive(TypedBuilder)]
pub(crate) struct TemporalQueryRequest {
    /// The tenant-scoped capabilities the query runs against.
    pub(crate) services: Arc<TemporalRouteServices>,
    /// The context branch the query reads.
    pub(crate) target: ContextTarget,
    /// The representation the response was negotiated to (clause 6.3.4).
    pub(crate) accept: AcceptFormatNoGeo,
    /// The clause 6.3.x protocol headers: the `NGSILD-EntityMap` the request
    /// runs under, the `Via` chain it arrived with, and the extension headers
    /// that add to the query.
    pub(crate) protocol: NgsiLdProtocolHeaders,
    /// The URI the client addressed, which names the `instance` of every
    /// problem the operation reports and anchors its pagination links.
    pub(crate) uri: Uri,
    /// The `apiRoot` a wrapper the response `Link` names is served under.
    pub(crate) api_root: ApiRoot,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub(crate) headers: HeaderMap,
    /// The JSON-LD context the query's terms expand under (clause 6.3.5).
    pub(crate) context: ContextSpec,
    /// The query itself, in the raw form the binding decoded it to.
    pub(crate) params: TemporalQueryParameters,
    /// Whether the binding's own request URI can carry the clause 6.3.10
    /// pointers to the neighbouring pages, which the query-string binding's
    /// can and the POST binding's cannot.
    pub(crate) pointers: PagePointers,
}

/// GET /ngsi-ld/v1/temporal/entities -- Query Temporal Evolution of Entities
/// carried in the request's query string (clause 6.18.3.2).
///
/// # Errors
///
/// The classified refusal of a grouping the deployment does not enable, and
/// every failure clause 5.7.4 raises while the query runs.
pub async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via: _, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    protocol: NgsiLdProtocolHeaders,
    AdmittedTemporalQuery { params, .. }: AdmittedTemporalQuery<GroupingGate>,
) -> Result<Response, ProblemResponse> {
    // Boxed: a temporal query parameter set is wide enough that the operation's
    // future overflows the task-local budget the `large_futures` lint guards.
    Box::pin(run(TemporalQueryRequest::builder()
        .services(services)
        .target(target)
        .accept(accept)
        .protocol(protocol)
        .uri(uri)
        .api_root(api_root)
        .headers(headers)
        .context(context)
        .params(params)
        // Clause 6.18.3.2 carries the whole query in the request URI, so a
        // pointer built from it retrieves the next page on its own.
        .pointers(PagePointers::OverRequestUri)
        .build()))
    .await
}
