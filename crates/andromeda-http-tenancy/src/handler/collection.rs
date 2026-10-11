//! `GET /ngsi-ld/v1/tenants` -- the paginated tenant collection.

use crate::{
    error::{TenancyHttpError, classify},
    options::TenantOptions,
    params::{TENANT_PAGE_SIZE, TenantListParams},
    state::TenancyRouteState,
    tenant_response::{build_tenant, tenant_page_start},
};
use andromeda_http_extraction::{pagination_policy::RequestPagination, query::ValidatedQuery};
use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_results_count::NgsiLdResultsCount};
use andromeda_http_response::{
    pagination::{PageOutcome, append_page_links},
    parameter_failure::classify_parameter_error,
    problem_response::ProblemResponse,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::tenant::{Tenant, tenant_id};
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_ngsi_ld_request::pagination::{
    page_pointers::PagePointers,
    page_request::{CollectionOrder, PageRequest},
};
use andromeda_tenancy::tenant_view::TenantView;
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::Uri as RequestUri,
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// List the named tenants as an array of `Tenant` resources.
///
/// Paged in tenant-name order by the broker's pagination policy: by opaque
/// cursor (clause 5.5.9.1) or by `limit`/`offset` (clause 5.5.9.2). The page
/// carries `next`/`last` `Link`s while more pages follow and `prev`/`first`
/// while earlier pages exist, and a single page carries no pagination `Link`
/// at all. The `NGSILD-Results-Count` header always reports the total, and the
/// same `options` keywords as the single-tenant read apply to every resource on
/// the page.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] for an unrecognised `options` keyword, a
/// pagination parameter the policy refuses or a cursor that addresses no
/// tenant, and when the administration or usage read fails.
pub async fn query(
    State(state): State<TenancyRouteState>,
    OriginalUri(uri): OriginalUri,
    RequestPagination { policy: pagination, context }: RequestPagination,
    ValidatedQuery(params): ValidatedQuery<TenantListParams>,
) -> Result<Response, ProblemResponse> {
    let options = TenantOptions::parse(params.options.as_deref()).map_err(|error| error.classify().with_instance(uri.clone()))?;
    // The collection always reports its total, so `limit=0` asks for that
    // total alone (Table 6.3.10-1).
    let window = pagination
        .window(
            &params.page,
            PageRequest {
                size: TENANT_PAGE_SIZE,
                order: CollectionOrder::Identifier,
                count: CountPolicy::Emit,
                pointers: PagePointers::OverRequestUri,
                context: &context,
            },
        )
        .map_err(|error| classify_parameter_error(&error).with_instance(uri.clone()))?;
    let start = tenant_page_start(&window.start).map_err(|report| report.with_instance(uri.clone()))?;

    let listing = state
        .administration()
        .list_tenants(&start, window.with_lookahead().limit)
        .await
        .map_err(|error| classify(&TenancyHttpError::from(error)).with_instance(uri.clone()))?;
    let (views, reach) = window.settle(listing.views);

    let mut tenants: Vec<Tenant> = Vec::with_capacity(views.len());
    for view in &views {
        tenants.push(
            build_tenant(view, options, state.usage())
                .await
                .map_err(|error| classify(&error).with_instance(uri.clone()))?,
        );
    }

    let boundary = collection_boundary(&views, &uri)?;
    let count = usize::try_from(listing.total).unwrap_or(usize::MAX);

    let mut response = Json(tenants).into_response();
    response.headers_mut().typed_insert(NgsiLdResultsCount::new(count));
    append_page_links(
        &mut response,
        &uri,
        &context,
        &window,
        PageOutcome {
            total: Some(listing.total),
            reach,
            boundary,
        },
        ResponseMediaType::Json,
    );
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
