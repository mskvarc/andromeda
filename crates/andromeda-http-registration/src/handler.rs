//! Handlers for the `/csourceRegistrations` endpoints.

use crate::{
    error::{RegistrationResultExt, internal_problem},
    state::{RegistrationTenantState, RegistrationTenantStateOrCreate},
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    context::ContextUrl,
    jsonld::JsonLdDocument,
    pagination_policy::RequestPagination,
    prefer::PreferHeader,
    query::ValidatedQuery,
    response_context_request::ResponseContextRequest,
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    headers::with_location,
    internal_failure_summary::InternalFailureSummary,
    pagination::{PageOutcome, append_page_links},
    problem_response::ProblemResponse,
    single::NgsiLdResponse,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, registration::RegistrationRenderer, response::ResponsePayload};
use andromeda_ngsi_ld_request::{
    empty::EmptyParams,
    pagination::{
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
    },
    query::{QUERY_PAGE_SIZE, csr::CsrQueryParameters},
};
use andromeda_registration::query::DiscoveredRegistrations;
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(crate) async fn create(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ProvisionableTenantState(state): RegistrationTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    JsonLdDocument { document: payload, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let identifier = state.registrations.create(payload, &context).await.with_registration_instance(&uri)?;
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/csourceRegistrations/{identifier}"),
    ))
}

pub(crate) async fn get(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    TypedPath(registration_id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let registration = state.registrations.get(&registration_id, &context).await.with_registration_instance(&uri)?;
    let payload = RegistrationRenderer::new(registration)
        .with_sys_attrs(params.parse_options().with_registration_instance(&uri)?.sys_attrs)
        .with_pick(params.pick.as_deref())
        .with_omit(params.omit.as_deref())
        .with_language_filter(params.lang.as_ref())
        .render();
    let rendered = ResponseContextSource::new(state.registrations.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_registration_instance(&uri)?
        .with_geometry_selection(GeometrySelection::new(params.geometry_property.as_ref(), &[]))
        .with_payload(ResponsePayload::Single(payload))
        .render()
        .with_registration_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

pub(crate) async fn query(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    // Clause 6.3.4: `application/geo+json` is acceptable only on Retrieve Entity
    // and Query Entities; every other operation answers 406. Query Context
    // Source Registrations is clause 5.10.2, and clause 5.10.2.5 gives its
    // output as a JSON-LD array of registrations, which has no GeoJSON form.
    accept: AcceptFormatNoGeo,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    IncomingVia(incoming_via): IncomingVia,
    RequestPagination(pagination): RequestPagination,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let options = params.parse_options().with_registration_instance(&uri)?;
    // Clause 5.10.2: a discovery always reports its total, so `limit=0` asks
    // for that total alone (Table 6.3.10-1). The registrations are paged in
    // identifier order in either pagination mode.
    let window = pagination
        .window(
            &params.page,
            PageRequest {
                size: QUERY_PAGE_SIZE,
                order: CollectionOrder::Identifier,
                count: CountPolicy::Emit,
                pointers: PagePointers::OverRequestUri,
            },
        )
        .with_registration_instance(&uri)?;
    // Clause 4.3.6.4: the `Via` chain lists the context sources the request
    // already passed through; clause 5.12 excludes their registrations.
    let encountered_sources: Vec<_> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect(); // clone: the discovery owns the listing across the store await
    let DiscoveredRegistrations {
        registrations,
        total,
        boundary,
        scanned,
    } = state
        .registrations
        .query(&params, &window, &encountered_sources, &context)
        .await
        .with_registration_instance(&uri)?;
    let registrations = registrations
        .into_iter()
        .map(|registration| {
            RegistrationRenderer::new(registration)
                .with_sys_attrs(options.sys_attrs)
                .with_pick(params.pick.as_deref())
                .with_omit(params.omit.as_deref())
                .with_language_filter(params.lang.as_ref())
                .render()
        })
        .collect::<Vec<_>>();
    let rendered = ResponseContextSource::new(state.registrations.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_registration_instance(&uri)?
        .with_geometry_selection(GeometrySelection::new(params.geometry_property.as_ref(), &[]))
        .with_payload(ResponsePayload::Collection(registrations))
        .render()
        .with_registration_instance(&uri)?;
    let total_count = usize::try_from(total).map_err(|error| {
        internal_problem(
            format!("registration count does not fit the response platform: {error}"),
            InternalFailureSummary::new("internal registration error"),
            &uri,
        )
    })?;
    let pagination_total = i64::try_from(total).map_err(|error| {
        internal_problem(
            format!("registration count does not fit offset pagination: {error}"),
            InternalFailureSummary::new("internal registration error"),
            &uri,
        )
    })?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_total_count(total_count).into_response();
    // Clause 5.5.9.1: the cursor pointers and the page-fullness signal are read
    // from the raw store page, before the clause-5.10.2.5 trim thinned it, so a
    // walk continues past every registration the store scanned.
    append_page_links(
        &mut response,
        &uri,
        &window,
        PageOutcome {
            total: Some(pagination_total),
            returned: scanned,
            boundary,
        },
        accept.media_type(),
    );
    Ok(response)
}

pub(crate) async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(registration_id): TypedPath<Identifier>,
    JsonLdDocument { document: payload, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    state
        .registrations
        .update(&registration_id, payload, &context)
        .await
        .with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(registration_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    state.registrations.delete(&registration_id).await.with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
