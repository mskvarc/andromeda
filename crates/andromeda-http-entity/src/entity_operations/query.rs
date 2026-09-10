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
    entity_operations::{
        AcceptFormat,
        EntityQueryParameters,
        EntityResultExt,
        EntityTenantState,
        JsonLd,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        PostQueryParams,
        ProblemResponse,
        Query,
        Response,
        ValidatedQuery,
    },
};
use andromeda_http_extraction::context_target;

pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via: _,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(url_params): ValidatedQuery<PostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    // Clause 6.3.5: a POST query carries its `@context` in the body as well as
    // in the `Link` headers, and the operation reads both.
    let context = context.merge(body_context);
    let mut query = EntityQueryParameters::try_from(request).with_entity_instance(&uri)?;
    apply_url_parameters(&mut query, url_params);
    run(EntityQueryRequest {
        services,
        target,
        accept,
        protocol,
        uri,
        headers,
        context,
        query,
        binding: QueryBinding::PostedBody,
    })
    .await
}

/// Fold the URL parameters over the clause 5.2.23 body the query arrived in.
///
/// The same parameter may be written in the URL or in the canonical body. The
/// specification does not say which wins, so this is broker convention: the URL
/// is the more specific of the two and takes precedence, but only where it
/// actually carries a value -- an absent query-string parameter leaves the
/// body's member standing rather than erasing it.
///
/// `limit`, `offset` and `count` are the exception, and nothing is overwritten
/// by them: Table 5.2.23-1 defines none of the three, so the body conversion
/// leaves each absent and the URL is the only place clauses 6.3.10 and 6.3.13
/// can put them.
fn apply_url_parameters(query: &mut EntityQueryParameters, url_params: PostQueryParams) {
    query.limit = url_params.limit;
    query.offset = url_params.offset;
    query.count = url_params.count;
    query.options = url_params.options.or(query.options.take());
    query.local = url_params.local.or(query.local);
    query.pick = url_params.pick.or(query.pick.take());
    query.omit = url_params.omit.or(query.omit.take());
    query.dataset_id = url_params.dataset_id.or(query.dataset_id.take());
    query.format = url_params.format.or(query.format.take());
    query.join = url_params.join.or(query.join);
    query.join_level = url_params.join_level.or(query.join_level);
    query.geometry_property = url_params.geometry_property.or(query.geometry_property.take());
    query.lang = url_params.lang.or(query.lang.take());
}
