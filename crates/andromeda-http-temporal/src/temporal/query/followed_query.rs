//! A followed pointer of a temporal query sent by POST, resolved to the query
//! the broker holds (clauses 5.5.9 and 6.24.3.1).
//!
//! Every pointer of a paged POST temporal query is a GET on
//! `/temporal/entities` naming the held query by its key. Following one runs
//! the held query exactly as the POST ran it: the same clause 5.2.23 body
//! under the same `@context` (clause 5.5.9.1), admitted as a body is, with the
//! URL parameters the pointer carries.

use crate::{
    error::TemporalResultExt,
    extension_gate::TemporalExtensionGate,
    request_context_admission::admit_request_context,
    state::TemporalRouteServices,
    temporal::query::page::QueriedPage,
};
use andromeda_http_extraction::{admitted_temporal_query::admitted_body_parameters, pagination_policy::RequestPagination};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    user_context_admission::{AdmittedContext, SubmittedContext},
};
use andromeda_ngsi_ld_request::{
    pagination::{page_pointers::PagePointers, posted_reference::PostedQueryReference},
    query::temporal::{TemporalQueryParameters, post_parameters::TemporalPostQueryParams},
};
use andromeda_posted_query::posted_query::PostedQueryKind;
use axum::http::Uri;

/// The held query a followed pointer names, ready to run.
pub(crate) struct FollowedQuery {
    /// The context the query was posted with, admitted (clause 5.5.7).
    pub(crate) context: AdmittedContext,
    /// The posted query with the URL parameters the pointer carries.
    pub(crate) params: TemporalQueryParameters,
    /// The element page the pointer asks for, of the walk the key names.
    pub(crate) page: QueriedPage,
}

/// Resolve the pointer `reference`, followed under the context `follower`
/// named, to the query the broker holds, admitted by `admission`.
///
/// # Errors
///
/// `BadRequestData` for a key that names no live query posted to Query
/// Temporal Evolution of Entities and for a follower naming another
/// `@context` (clause 5.5.9.1); otherwise the classified failure of the store,
/// of admitting the held context or the held query, or of the page asked for.
pub(crate) async fn followed_query(
    services: &TemporalRouteServices,
    reference: PostedQueryReference<TemporalPostQueryParams>,
    admission: &TemporalExtensionGate,
    follower: &ContextSpec,
    pagination: RequestPagination,
    uri: &Uri,
) -> Result<FollowedQuery, ProblemResponse> {
    let PostedQueryReference { key, url } = reference;
    let posted = services.posted_queries.resolve(&key, PostedQueryKind::Temporal).await.with_instance(uri)?;
    posted.admit_follower(follower).with_instance(uri)?;
    let (body, context) = posted.into_parts();
    let context = admit_request_context(services.temporal.engine(), SubmittedContext::from(context), uri).await?;
    let mut params = admitted_body_parameters(body, url, admission).map_err(|report| ProblemResponse::classified(report, uri.clone()))?;
    // Clause 5.5.9.1: a followed cursor is bound to the query it walks, which
    // the key names.
    params.page.query = Some(key.clone()); // clone: the walk is paged under the same key the cursor is bound to
    let page = QueriedPage::requested(pagination.following(key), &params, PagePointers::OverPostedQuery).with_instance(uri)?;
    Ok(FollowedQuery { context, params, page })
}
