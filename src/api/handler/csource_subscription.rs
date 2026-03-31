use crate::{
    api::{
        error::ApiError,
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, params::ValidatedQuery},
        header::{link_from_values, link_header_value, link_value, media_type},
        response::ngsild::{NgsiLdCollectionResponse, NgsiLdResponse},
        tenant::TenantState,
    },
    jsonld::ContextSpec,
};
use axum::{
    body::Bytes,
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use mediatype::{MediaTypeBuf, names};
use serde::Deserialize;
use serde_json::Value;
use url::form_urlencoded;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub page: Option<i64>,
    pub options: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct ListOptions {
    count: bool,
    sys_attrs: bool,
}

fn parse_list_options(options: Option<&str>, uri: &axum::http::Uri) -> Result<ListOptions, ApiError> {
    let mut parsed = ListOptions::default();
    if let Some(opts) = options {
        for opt in opts.split(',').map(str::trim) {
            if opt.is_empty() {
                continue;
            }
            match opt {
                "count" => parsed.count = true,
                "sysAttrs" => parsed.sys_attrs = true,
                _ => {
                    return Err(ApiError::from_error(
                        crate::error::AndromedaError::invalid_request(format!("Invalid options value: '{}'", opt)),
                        uri.clone(),
                    ));
                }
            }
        }
    }
    Ok(parsed)
}

fn strip_subscription_sys_attrs(mut value: Value) -> Value {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("createdAt");
        obj.remove("modifiedAt");
        obj.remove("jsonldContext");
        // isActive is only shown when explicitly false (paused); true is the default
        if obj.get("isActive").and_then(|v| v.as_bool()) == Some(true) {
            obj.remove("isActive");
        }
        if let Some(notification) = obj.get_mut("notification")
            && let Some(notif_obj) = notification.as_object_mut()
        {
            notif_obj.remove("timesSent");
            notif_obj.remove("lastNotification");
            notif_obj.remove("lastSuccess");
            notif_obj.remove("lastFailure");
        }
    }
    value
}

fn build_page_link(uri: &axum::http::Uri, limit: i64, page: i64) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(query) = uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "limit" || k == "offset" || k == "page" {
                continue;
            }
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    pairs.push(("limit".to_string(), limit.to_string()));
    pairs.push(("page".to_string(), page.to_string()));

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

    let current_page = (offset / limit) + 1;
    let total_pages = (total + limit - 1) / limit;

    let mut links = Vec::new();

    if current_page > 1 {
        links.push(("prev", build_page_link(uri, limit, current_page - 1)));
    }
    if current_page < total_pages {
        links.push(("next", build_page_link(uri, limit, current_page + 1)));
    }

    let values = links
        .into_iter()
        .map(|(rel, target)| link_value(&target, Some(rel), Some(media_type::JSON_LD)))
        .collect::<Vec<_>>();
    if let Some(link) = link_from_values(values)
        && let Some(value) = link_header_value(&link)
    {
        response.headers_mut().append("Link", value);
    }
}

/// Check Content-Type header for JSON compatibility.
/// Returns `Ok(())` if JSON or absent/unparseable (we'll try to parse the body),
/// `Err(())` if explicitly a non-JSON media type.
fn check_json_content_type(headers: &HeaderMap) -> Result<(), ()> {
    let Some(raw) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let Ok(mt) = raw.parse::<MediaTypeBuf>() else {
        return Ok(());
    };
    if mt.ty() != names::APPLICATION {
        return Err(());
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Ok(());
    }
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return Ok(());
    }
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Ok(());
    }
    Err(())
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    check_json_content_type(headers).is_ok()
}

fn extract_body_context(payload: &Value) -> Option<Value> {
    payload.as_object().and_then(|obj| obj.get("@context")).cloned()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {}

pub async fn create(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ValidatedQuery(_params): ValidatedQuery<CreateParams>,
    context: ContextUrl,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !is_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from_error(crate::error::AndromedaError::invalid_request(format!("Invalid JSON: {}", e)), uri.clone()))?;
    let service = &state.csr_subscriptions;
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    let id = service.create(payload, &context, None).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", id)]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let subscription = service
        .get_for_response(&subscription_id, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let subscription = if options.sys_attrs {
        subscription
    } else {
        strip_subscription_sys_attrs(subscription)
    };

    let response = NgsiLdResponse::ok(subscription)
        .with_context(context.clone())
        .with_format(accept.format())
        .into_response();
    Ok(response)
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let limit = params.limit.unwrap_or(100);
    if limit <= 0 {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("limit must be greater than 0"),
            uri.clone(),
        ));
    }
    if let Some(page) = params.page
        && page <= 0
    {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("page must be greater than 0"),
            uri.clone(),
        ));
    }
    let offset = if let Some(page) = params.page {
        (page - 1) * limit
    } else {
        let offset = params.offset.unwrap_or(0);
        if offset < 0 {
            return Err(ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("offset must be greater than or equal to 0"),
                uri.clone(),
            ));
        }
        offset
    };

    let (mut subscriptions, total) = service
        .list_for_response(Some(limit), Some(offset), &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if !options.sys_attrs {
        subscriptions = subscriptions.into_iter().map(strip_subscription_sys_attrs).collect();
    }

    let mut response = NgsiLdCollectionResponse::ok(subscriptions)
        .with_context(context.clone())
        .with_format(accept.format());

    if options.count {
        response = response.with_total_count(total as usize);
    }

    let mut response = response.into_response();
    append_pagination_links(&mut response, &uri, limit, offset, total);
    Ok(response)
}

pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if check_json_content_type(&headers).is_err() {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from_error(crate::error::AndromedaError::bad_request_data(format!("Invalid JSON: {}", e)), uri.clone()))?;
    let service = &state.csr_subscriptions;
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    service
        .update(&subscription_id, payload, &context, None)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    service.delete(&subscription_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
