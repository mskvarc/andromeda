//! The POST binding of "Query Temporal Evolution of Entities" (clause 6.24.3.1).
//!
//! Clause 6.24.3.1: "This method is associated to the operation 'Query Temporal
//! Evolution of Entities' and shall exhibit the behaviour defined by clause
//! 5.7.4", and clause 6.24.1 adds that "the behaviour of this clause mirrors the
//! one in clause 6.18.3.2, which performs the [same] operation by means of a GET
//! method". The only thing this binding owns is therefore the decoding: a clause
//! 5.2.23 body for the query, and the clause 6.3.10 to 6.3.13 URL parameters the
//! body has no member for. The operation itself is
//! [`crate::temporal::query::run`], the same one the GET binding runs.

use crate::{
    error::TemporalResultExt,
    state::TemporalTenantState,
    temporal::query::{TemporalQueryRequest, run},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context_target,
    jsonld::JsonLd,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
};
use andromeda_http_response::{pagination::PagePointers, problem_response::ProblemResponse};
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, post_parameters::TemporalPostQueryParams};
use axum::response::Response;

/// POST /ngsi-ld/v1/temporal/entityOperations/query -- Query Temporal Evolution
/// of Entities carried in a clause 5.2.23 `Query` body.
///
/// # Errors
///
/// The classified failure of converting the body into the operation's inputs,
/// and every failure clause 5.7.4 raises while the query runs.
pub async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: TemporalTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via: _,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(url_params): ValidatedQuery<TemporalPostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: a POST query carries its `@context` in the body as well as
    // in the `Link` headers, and the operation reads both.
    let context = context.merge(body_context);
    let mut params = TemporalQueryParameters::try_from(request).with_instance(&uri)?;
    url_params.apply_to(&mut params);
    // Boxed: a temporal query parameter set is wide enough that the operation's
    // future overflows the task-local budget the `large_futures` lint guards.
    Box::pin(run(TemporalQueryRequest {
        services,
        target,
        accept,
        protocol,
        uri,
        headers,
        context,
        params,
        // Clause 6.3.10 asks for a Link Target "that could be dereferenced by
        // an NGSI-LD Client to retrieve the next page of NGSI-LD Elements", and
        // clause 5.5.9.1 that the pointers "contain all the parameters needed
        // to allow NGSI-LD Clients to retrieve the next and previous page,
        // without further interactions with the API". This binding's query is
        // the clause 5.2.23 body of a POST-only resource: no URI-reference
        // carries it, and one naming the resource with `limit` and `offset`
        // alone would answer 405 to the GET a client dereferences it with.
        // Clause 4.12's "mechanism to flag NGSI-LD Clients when there are
        // remaining NGSI-LD Elements" is served here by the clause 6.3.13
        // `NGSILD-Results-Count` a counted query asks for.
        pointers: PagePointers::NotDereferenceable,
    }))
    .await
}
