//! The POST binding of "Query Entities" (clause 6.23.3.1).
//!
//! Clause 6.23.3.1: "This method is associated to the operation 'Query
//! Entities' and shall exhibit the behaviour defined by clause 5.7.2", and
//! clause 6.23.1 adds that "the behaviour of this clause mirrors the one in
//! clause 6.4.3.2, which performs the 'Query Entity' operation [...] by means of
//! a GET method". The operation itself is [`crate::entity::query::run`], the
//! same one the GET binding runs, which is what makes Table 6.23.3.1-1's `201
//! Created` and its `NGSILD-EntityMap` header answer here too.
//!
//! Clause 5.7.2.4 asks of the operation that "pagination logic shall be in
//! place as mandated by clause 5.5.9", and clause 6.3.10 has every page link be
//! "a URI-reference that could be dereferenced by an NGSI-LD Client". The query
//! is in the body, which no URI-reference carries, so the binding hands the
//! operation the query as a posted query: the broker holds it when the page
//! points at another page, and every pointer names it by its digest on the GET
//! binding.

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
use andromeda_posted_query::posted_query::{PostedQuery, PostedQueryKind};

/// Query Entities through its POST binding, the query read from the clause 5.2.23 body.
pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope { uri, incoming_via: _, headers }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    protocol: NgsiLdProtocolHeaders,
    pagination: RequestPagination,
    // Clause 6.3.5: the query is read against the one context the body
    // extractor resolved: the body's own `@context` for `application/ld+json`,
    // the `Link` context for `application/json`.
    EntityPostQuery { body, url, context }: EntityPostQuery,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(services.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let body = body.admitted(&context).await?;
    // clone: the held query keeps the context it was read under, which the operation goes on reading it under
    let posted = PostedQuery::new(PostedQueryKind::Entities, body, context.request_context().clone()).with_entity_instance(&uri)?;
    // clone: the held query keeps its body, while the operation consumes its own reading of it
    let query = url.parameters_of(posted.query().clone()).with_entity_instance(&uri)?;
    // clone: the binding holds the posted query, whose digest the walk is paged under
    let pagination = pagination.posted(posted.digest().clone()).with_entity_instance(&uri)?;
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
        .binding(QueryBinding::PostedBody(Box::new(posted)))
        .pagination(pagination)
        .build())
    .await
}
