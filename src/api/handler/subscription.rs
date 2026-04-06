use crate::{
    api::{
        response::ngsild::{NgsiLdCollectionResponse, NgsiLdResponse},
        tenant::TenantState,
    },
    error::api::ApiError,
    http::{
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, query::ValidatedQuery},
        header::link::{Link, LinkValue},
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
        // isActive is only shown when explicitly false (paused); true is the default
        if obj.get("isActive").and_then(|v| v.as_bool()) == Some(true) {
            obj.remove("isActive");
        }
        // localOnly is only shown when true; false is the default
        if obj.get("localOnly").and_then(|v| v.as_bool()) != Some(true) {
            obj.remove("localOnly");
        }
    }
    value
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

    let mut links = Vec::new();

    if offset > 0 {
        let prev_offset = (offset - limit).max(0);
        links.push(("prev", build_page_link(uri, limit, prev_offset)));
    }
    if offset + limit < total {
        links.push(("next", build_page_link(uri, limit, offset + limit)));
    }

    let values = links
        .into_iter()
        .map(|(rel, target)| LinkValue::with_params(&target, Some(rel), Some(LinkValue::JSON_LD_MEDIA_TYPE)))
        .collect::<Vec<_>>();
    if let Some(link) = Link::from_values(values)
        && let Some(value) = link.to_header_value()
    {
        response.headers_mut().append("Link", value);
    }
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    let raw = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let Ok(mt) = raw.parse::<MediaTypeBuf>() else {
        return false;
    };
    if mt.ty() != names::APPLICATION {
        return false;
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return true;
    }
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return true;
    }
    mt.subty() == names::JSON && mt.suffix().is_none()
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
    let service = &state.subscriptions;
    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    let id = service
        .create(payload, &context, base_url.as_deref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/subscriptions/{}", id))]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let subscription = service
        .get_for_response(&subscription_id, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let mut subscription = if options.sys_attrs {
        subscription
    } else {
        strip_subscription_sys_attrs(subscription)
    };

    // Extract jsonldContext from body and use it for the response context
    let response_context = if let Some(jsonld_ctx) = subscription.as_object_mut().and_then(|obj| obj.remove("jsonldContext")) {
        if let Some(url) = jsonld_ctx.as_str() {
            ContextSpec::from_links(vec![url.to_string()])
        } else {
            context
        }
    } else {
        context
    };

    let response = NgsiLdResponse::ok(subscription)
        .with_context(response_context)
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
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    if limit <= 0 {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("limit must be greater than 0"),
            uri.clone(),
        ));
    }
    if offset < 0 {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("offset must be greater than or equal to 0"),
            uri.clone(),
        ));
    }

    let (mut subscriptions, total) = service
        .list_for_response(Some(limit), Some(offset), &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if !options.sys_attrs {
        subscriptions = subscriptions.into_iter().map(strip_subscription_sys_attrs).collect();
    }

    // Strip jsonldContext from each subscription body
    for sub in &mut subscriptions {
        if let Some(obj) = sub.as_object_mut() {
            obj.remove("jsonldContext");
        }
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
    if !is_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from_error(crate::error::AndromedaError::invalid_request(format!("Invalid JSON: {}", e)), uri.clone()))?;
    let service = &state.subscriptions;
    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    service
        .update(&subscription_id, payload, &context, base_url.as_deref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, ApiError> {
    let service = &state.subscriptions;
    service.delete(&subscription_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
