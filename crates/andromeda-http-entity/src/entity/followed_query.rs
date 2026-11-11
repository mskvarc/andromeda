//! A followed pointer of a query sent by POST, resolved to the query the broker
//! holds (clauses 5.5.9 and 6.23.3.1).
//!
//! Every pointer of a paged POST query is a GET on `/entities` naming the held
//! query by its key. Following one runs the held query exactly as the POST ran
//! it: the same clause 5.2.23 body under the same `@context` (clause 5.5.9.1),
//! with the URL parameters the pointer carries.

use crate::{entity::EntityResultExt, state::EntityRouteServices};
use andromeda_http_extraction::pagination_policy::RequestPagination;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
};
use andromeda_ngsi_ld_request::{
    entity_operations::PostQueryParams,
    pagination::posted_reference::PostedQueryReference,
    query::entity::parameters::EntityQueryParameters,
};
use andromeda_posted_query::posted_query::PostedQueryKind;
use axum::http::Uri;

/// The held query a followed pointer names, ready to run.
pub(crate) struct FollowedQuery {
    /// The context the query was posted with, admitted (clause 5.5.7).
    pub(crate) context: AdmittedContext,
    /// The posted query with the URL parameters the pointer carries.
    pub(crate) query: EntityQueryParameters,
    /// The pagination of the walk, paged under the key the pointer names.
    pub(crate) pagination: RequestPagination,
}

/// Resolve the pointer `reference`, followed under the context `follower`
/// named, to the query the broker holds.
///
/// # Errors
///
/// `BadRequestData` for a key that names no live query posted to Query
/// Entities and for a follower naming another `@context` (clause 5.5.9.1);
/// otherwise the classified failure of the store, of admitting the held
/// context, or of reading the held query.
pub(crate) async fn followed_query(
    services: &EntityRouteServices,
    reference: PostedQueryReference<PostQueryParams>,
    follower: &ContextSpec,
    pagination: RequestPagination,
    uri: &Uri,
) -> Result<FollowedQuery, ProblemResponse> {
    let PostedQueryReference { key, url } = reference;
    let posted = services
        .posted_queries
        .resolve(&key, PostedQueryKind::Entities)
        .await
        .with_entity_instance(uri)?;
    posted.admit_follower(follower).with_entity_instance(uri)?;
    let (body, context) = posted.into_parts();
    let context = admit_user_context(services.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_instance(uri)?;
    let mut query = url.parameters_of(body).with_entity_instance(uri)?;
    // Clause 5.5.9.1: a followed cursor is bound to the query it walks, which
    // the key names.
    query.page.query = Some(key.clone()); // clone: the walk is paged under the same key the cursor is bound to
    Ok(FollowedQuery {
        context,
        query,
        pagination: pagination.following(key),
    })
}
