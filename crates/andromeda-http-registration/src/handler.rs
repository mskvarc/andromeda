//! Handlers for the `/csourceRegistrations` endpoints.

use crate::{
    error::{RegistrationResultExt, internal_problem},
    state::{RegistrationTenantState, RegistrationTenantStateOrCreate},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    query::ValidatedQuery,
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    headers::with_location,
    internal_failure_summary::InternalFailureSummary,
    pagination::{CursorPointers, PageExtent, PageLinks, append_pagination_links},
    problem_response::ProblemResponse,
    render::start_rendering_normalized,
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, registration::RegistrationRenderer, response::ResponsePayload};
use andromeda_ngsi_ld_request::{
    pagination_cursor::decode_cursor,
    query::{SharedQueryParams, csr::CsrQueryParameters},
};
use andromeda_registration::query::DiscoveredRegistrations;
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

pub(crate) async fn create(
    ProvisionableTenantState(state): RegistrationTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let identifier = state
        .registrations
        .create(payload, &context.merge(body_context))
        .await
        .with_registration_instance(&uri)?;
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/csourceRegistrations/{identifier}"),
    ))
}

pub(crate) async fn get(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    let registration = state.registrations.get(&registration_id, &context).await.with_registration_instance(&uri)?;
    let payload = RegistrationRenderer::new(registration)
        .with_sys_attrs(params.parse_options().with_registration_instance(&uri)?.sys_attrs)
        .with_pick(params.pick.as_deref())
        .with_omit(params.omit.as_deref())
        .with_language_filter(params.lang.as_ref())
        .render();
    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
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
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options().with_registration_instance(&uri)?;
    let limit = i64::from(params.effective_limit());
    let offset = i64::from(params.effective_offset());
    // Clause 5.5.9.1: decode the opaque pagination cursor so a malformed one is
    // a BadRequestData here; its mutual exclusion with offset is held by
    // `CsrQueryParameters::validate`.
    let incoming_cursor = decode_cursor(params.cursor.as_deref()).with_registration_instance(&uri)?;
    // Clause 4.3.6.4: the `Via` chain lists the context sources the request
    // already passed through; clause 5.12 excludes their registrations.
    let encountered_sources: Vec<_> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect(); // clone: the discovery owns the listing across the store await
    let DiscoveredRegistrations {
        registrations,
        total,
        boundary,
        page_full,
    } = state
        .registrations
        .query(&params, &encountered_sources, &context)
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
    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
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
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_total_count(total_count).into_response();
    // Clause 5.5.9.1: a discovery in the default id order walks by opaque cursor
    // by default; an explicit `offset` keeps the clause 5.5.9.2 offset pointers.
    // `offset` and `cursor` are mutually exclusive (validated), so their absence
    // together is the fresh first page of a cursor walk.
    if params.offset.is_some() {
        let pagination_total = i64::try_from(total).map_err(|error| {
            internal_problem(
                format!("registration count does not fit offset pagination: {error}"),
                InternalFailureSummary::new("internal registration error"),
                &uri,
            )
        })?;
        let links = PageLinks::Offset {
            offset,
            extent: PageExtent::Total(pagination_total),
        };
        append_pagination_links(&mut response, &uri, limit, links, accept.media_type());
    } else if let Some((first, last)) = boundary {
        let neighbours = PageCursor::neighbours(incoming_cursor.as_ref(), first, last, page_full);
        let links = PageLinks::Cursor(CursorPointers {
            prev: neighbours.prev.map(|cursor| cursor.encode()),
            next: neighbours.next.map(|cursor| cursor.encode()),
            last: Some(neighbours.last.encode()),
        });
        append_pagination_links(&mut response, &uri, limit, links, accept.media_type());
    }
    Ok(response)
}

pub(crate) async fn update(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    state
        .registrations
        .update(&registration_id, payload, &context.merge(body_context))
        .await
        .with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn delete(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(registration_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    state.registrations.delete(&registration_id).await.with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
