use crate::{
    api::{
        error::ApiError,
        header::{link_from_values, link_header_value, link_value},
        response::ngsild::{NgsiLdCollectionResponse, NgsiLdResponse},
        tenant::TenantState,
    },
    jsonld::ContextSpec,
};
use andromeda_extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, params::ValidatedQuery};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ngsi_ld_query::csr::CsrQueryParameters;
use serde_json::Value;
use url::{Url, form_urlencoded};
use urn::Urn;

fn parse_csr_id(id: &str, uri: &axum::http::Uri) -> Result<(), ApiError> {
    if id.starts_with("urn:") {
        id.parse::<Urn>().map_err(|_| {
            ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("Invalid registration id: not a valid URI"),
                uri.clone(),
            )
        })?;
    } else {
        Url::parse(id).map_err(|_| {
            ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("Invalid registration id: not a valid URI"),
                uri.clone(),
            )
        })?;
    }
    Ok(())
}

fn strip_csr_sys_attrs(mut value: Value) -> Value {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("status");
        obj.remove("timesSent");
        obj.remove("timesFailed");
        obj.remove("lastSuccess");
        obj.remove("lastFailure");
        obj.remove("createdAt");
        obj.remove("modifiedAt");
    }
    value
}

pub async fn create(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    let service = &state.csr;
    let context = context.merge(body_context);

    let id = service.create(payload, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/csourceRegistrations/{}", id))]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let csr = service.get(&registration_id, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let csr = if options.sys_attrs { csr } else { strip_csr_sys_attrs(csr) };

    Ok(NgsiLdResponse::ok(csr).with_context(context).with_format(accept.format()).into_response())
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ApiError> {
    use ngsi_ld_query::params::SharedQueryParams;

    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let limit = params.effective_limit() as i64;
    let offset = params.effective_offset() as i64;

    let (mut csrs, total) = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    if !options.sys_attrs {
        csrs = csrs.into_iter().map(strip_csr_sys_attrs).collect();
    }

    let response = NgsiLdCollectionResponse::ok(csrs)
        .with_context(context)
        .with_format(accept.format())
        .with_total_count(total as usize);

    let mut response = response.into_response();
    append_pagination_links(&mut response, &uri, limit, offset, total);
    Ok(response)
}

pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context = context.merge(body_context);
    service
        .update(&registration_id, payload, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(registration_id): Path<String>) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    service.delete(&registration_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

fn build_page_link(uri: &axum::http::Uri, limit: i64, offset: i64) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(query) = uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "limit" || k == "offset" {
                continue;
            }
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    pairs.push(("limit".to_string(), limit.to_string()));
    pairs.push(("offset".to_string(), offset.to_string()));

    let query = form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();

    if query.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), query)
    }
}

fn append_pagination_links(response: &mut Response, uri: &axum::http::Uri, limit: i64, offset: i64, total: i64) {
    if limit <= 0 || total <= 0 {
        return;
    }

    let last_offset = ((total - 1) / limit) * limit;

    let mut links = Vec::new();
    links.push(("first", build_page_link(uri, limit, 0)));
    links.push(("last", build_page_link(uri, limit, last_offset)));

    if offset > 0 {
        let prev_offset = (offset - limit).max(0);
        links.push(("prev", build_page_link(uri, limit, prev_offset)));
    }
    if offset + limit < total {
        links.push(("next", build_page_link(uri, limit, offset + limit)));
    }

    let values = links.into_iter().map(|(rel, target)| link_value(&target, Some(rel), None)).collect::<Vec<_>>();
    if let Some(link) = link_from_values(values) {
        if let Some(value) = link_header_value(&link) {
            response.headers_mut().append("Link", value);
        }
    }
}
