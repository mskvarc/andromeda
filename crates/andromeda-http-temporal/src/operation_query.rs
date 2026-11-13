//! The POST binding of "Query Temporal Evolution of Entities" (clause 6.24.3.1).
//!
//! Clause 6.24.3.1: "This method is associated to the operation 'Query Temporal
//! Evolution of Entities' and shall exhibit the behaviour defined by clause
//! 5.7.4", and clause 6.24.1 adds that "the behaviour of this clause mirrors the
//! one in clause 6.18.3.2, which performs the [same] operation by means of a GET
//! method". The operation itself is [`crate::temporal::query::run`], the same
//! one the GET binding runs.
//!
//! Clause 6.3.10 has every page link be "a URI-reference that could be
//! dereferenced by an NGSI-LD Client", and clause 5.5.9.1 has the pointers
//! "contain all the parameters needed to allow NGSI-LD Clients to retrieve the
//! next and previous page, without further interactions with the API". The
//! query is in the body, which no URI-reference carries, so the binding hands
//! the operation the query as a posted query: the broker holds it when the page
//! points at another page, and every pointer names it by its digest on the GET
//! binding.

use crate::{
    error::TemporalResultExt,
    extension_gate::TemporalExtensionGate,
    request_context_admission::admit_request_context,
    state::TemporalTenantState,
    temporal::query::{page::QueriedPage, request::TemporalQueryRequest, run},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    admitted_temporal_query::{AdmittedTemporalPostQuery, admitted_body_parameters},
    api_root::ApiRoot,
    context_target,
    pagination_policy::RequestPagination,
    protocol::NgsiLdProtocolHeaders,
    request_envelope::NgsiLdRequestEnvelope,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_request::pagination::page_pointers::PagePointers;
use andromeda_posted_query::posted_query::{PostedQuery, PostedQueryKind};
use axum::response::Response;

/// POST /ngsi-ld/v1/temporal/entityOperations/query: Query Temporal Evolution
/// of Entities carried in a clause 5.2.23 `Query` body.
///
/// # Errors
///
/// The classified failure of converting the body into the operation's inputs,
/// and every failure clause 5.7.4 raises while the query runs.
pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: TemporalTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via: _, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    protocol: NgsiLdProtocolHeaders,
    pagination: RequestPagination,
    AdmittedTemporalPostQuery { body, url, admission, context }: AdmittedTemporalPostQuery<TemporalExtensionGate>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.7: the request `@context` is admitted before the query is
    // forwarded or anything is expanded against it.
    let context = admit_request_context(services.temporal.engine(), context, &uri).await?;
    let body = body.admitted(&context).await?;
    // clone: the held query keeps the context it was read under, which the operation goes on reading it under
    let posted = PostedQuery::new(PostedQueryKind::Temporal, body, context.request_context().clone()).with_instance(&uri)?;
    // clone: the held query keeps its body, while the operation consumes its own reading of it
    let params = admitted_body_parameters(posted.query().clone(), url, &admission).map_err(|report| ProblemResponse::classified(report, uri.clone()))?;
    // clone: the request holds the posted query, whose digest the walk is paged under
    let pagination = pagination.posted(posted.digest().clone()).with_instance(&uri)?;
    let page = QueriedPage::requested(pagination, &params, PagePointers::OverPostedQuery).with_instance(&uri)?;
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
        .page(page)
        .posted(Some(Box::new(posted)))
        .build()))
    .await
}
