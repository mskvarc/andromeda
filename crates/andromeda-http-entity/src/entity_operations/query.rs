//! The POST binding of "Query Entities" (clause 6.23.3.1).
//!
//! Clause 6.23.3.1: "This method is associated to the operation 'Query
//! Entities' and shall exhibit the behaviour defined by clause 5.7.2", and
//! clause 6.23.1 adds that "the behaviour of this clause mirrors the one in
//! clause 6.4.3.2, which performs the 'Query Entity' operation [...] by means of
//! a GET method". The only thing this binding owns is therefore the decoding: a
//! clause 5.2.23 body for the query, and the URL parameters the body has no
//! member for. The operation itself is [`crate::entity::query::run`], the same
//! one the GET binding runs, which is what makes Table 6.23.3.1-1's `201
//! Created` and its `NGSILD-EntityMap` header answer here too.

use crate::{
    entity::{
        query::{EntityQueryRequest, run},
        query_binding::QueryBinding,
    },
    entity_operations::{AcceptFormat, ApiRoot, EntityTenantState, NgsiLdProtocolHeaders, NgsiLdRequestEnvelope, ProblemResponse, Response},
    error::EntityResultExt,
};
use andromeda_http_extraction::{context_target, entity_post_query::EntityPostQuery, pagination_policy::RequestPagination};
use andromeda_jsonld::user_context_admission::admit_user_context;

pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope { uri, incoming_via: _, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    protocol: NgsiLdProtocolHeaders,
    RequestPagination(pagination): RequestPagination,
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved -- the body's own `@context` for `application/ld+json`,
    // the `Link` context for `application/json`.
    EntityPostQuery { query, context }: EntityPostQuery,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(services.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let query = query.admitted(&context)?;
    run(EntityQueryRequest::builder()
        .services(services)
        .target(target)
        .accept(accept)
        .protocol(protocol)
        .uri(uri)
        .api_root(api_root)
        .headers(headers)
        .context(context)
        .query(query)
        .binding(QueryBinding::PostedBody)
        .pagination(pagination)
        .build())
    .await
}
