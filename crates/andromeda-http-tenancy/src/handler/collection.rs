//! `GET /ngsi-ld/v1/tenants` -- the paginated tenant collection.

use crate::{
    error::{TenancyHttpError, classify},
    options::TenantOptions,
    params::TenantListParams,
    state::TenancyRouteState,
    tenant_response::{build_tenant, keyset_from_cursor},
};
use andromeda_http_extraction::{classification::classify_parameter_error, query::ValidatedQuery};
use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_results_count::NgsiLdResultsCount};
use andromeda_http_response::{
    pagination::{CursorPointers, PageLinks, append_pagination_links},
    problem_response::ProblemResponse,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::tenant::{Tenant, tenant_id};
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_ngsi_ld_request::pagination_cursor::decode_cursor;
use andromeda_tenancy::tenant_view::TenantView;
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::Uri as RequestUri,
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// The default tenant-collection page size.
const DEFAULT_LIMIT: u32 = 100;

/// List the named tenants as an array of `Tenant` resources.
///
/// Cursor-paginated by tenant name (clause 5.5.9.1): the page carries `next`/
/// `last` `Link`s while more pages follow and `prev`/`first` while earlier pages
/// exist, and a single page carries no pagination `Link` at all. The
/// `NGSILD-Results-Count` header always reports the total, and the same `options`
/// keywords as the single-tenant read apply to every resource on the page.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] for an unrecognised `options` keyword or
/// pagination cursor, and when the administration or usage read fails.
pub async fn query(
    State(state): State<TenancyRouteState>,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<TenantListParams>,
) -> Result<Response, ProblemResponse> {
    let options = TenantOptions::parse(params.options.as_deref()).map_err(|error| error.classify().with_instance(uri.clone()))?;
    let incoming = decode_cursor(params.cursor.as_deref()).map_err(|error| classify_parameter_error(&error).with_instance(uri.clone()))?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
    let keyset = keyset_from_cursor(incoming.as_ref()).map_err(|report| report.with_instance(uri.clone()))?;

    let listing = state
        .administration()
        .list_tenants(&keyset, i64::from(limit))
        .await
        .map_err(|error| classify(&TenancyHttpError::from(error)).with_instance(uri.clone()))?;

    let mut tenants: Vec<Tenant> = Vec::with_capacity(listing.views.len());
    for view in &listing.views {
        tenants.push(
            build_tenant(view, options, state.usage())
                .await
                .map_err(|error| classify(&error).with_instance(uri.clone()))?,
        );
    }

    let boundary = collection_boundary(&listing.views, &uri)?;
    let returned = listing.views.len();
    let count = usize::try_from(listing.total).unwrap_or(usize::MAX);

    let mut response = Json(tenants).into_response();
    response.headers_mut().typed_insert(NgsiLdResultsCount::new(count));
    if let Some(links) = cursor_links(incoming.as_ref(), boundary, returned, limit) {
        append_pagination_links(&mut response, &uri, i64::from(limit), links, ResponseMediaType::Json);
    }
    Ok(response)
}

/// The identifiers of the first and last tenant on the page, from which the
/// cursor pointers are built, or `None` for an empty page.
fn collection_boundary(views: &[TenantView], uri: &RequestUri) -> Result<Option<(Identifier, Identifier)>, ProblemResponse> {
    match (views.first(), views.last()) {
        (Some(first), Some(last)) => {
            let first_id = tenant_id(&first.record.tenant_name).map_err(|error| classify(&TenancyHttpError::from(error)).with_instance(uri.clone()))?;
            let last_id = tenant_id(&last.record.tenant_name).map_err(|error| classify(&TenancyHttpError::from(error)).with_instance(uri.clone()))?;
            Ok(Some((first_id, last_id)))
        }
        _ => Ok(None),
    }
}

/// The pagination pointers for the page, or `None` when the page is empty.
///
/// A page that filled the requested limit is treated as having a further page,
/// which is how a keyset walk with no total decides `next` exists. Clause
/// 5.5.9.1's self-pointing suppression is enforced downstream by
/// [`append_pagination_links`].
fn cursor_links(incoming: Option<&PageCursor>, boundary: Option<(Identifier, Identifier)>, returned: usize, limit: u32) -> Option<PageLinks> {
    let (first, last) = boundary?;
    let full = returned == usize::try_from(limit).unwrap_or(usize::MAX);
    let neighbours = PageCursor::neighbours(incoming, first, last, full);
    Some(PageLinks::Cursor(CursorPointers::from(neighbours)))
}
