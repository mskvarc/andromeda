use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    domain::entity::EntityTypeValue,
    error::AndromedaError,
    http::{
        error::{batch::BatchOperationResult, problem::NgsiLdError},
        extractor::{
            accept::AcceptFormat,
            batch::{BatchBody, parse_batch_entities},
            context::ContextUrl,
            jsonld::JsonLd,
            prefer::PreferHeader,
            protocol::NgsiLdProtocolHeaders,
            query::ValidatedQuery,
            via::IncomingVia,
        },
        model::query_request::QueryRequest,
        parameter::{
            entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
            protocol::apply_entity_query_extension_headers,
        },
        response::{batch::NgsiLdBatchResponse, collection::NgsiLdCollectionResponse},
    },
    query::entity::EntityQueryParameters,
    renderer::{entity::EntityRenderer, response::ResponseRenderer},
    service::{
        context_source_forwarding::{ForwardBatchProvisionRequest, ForwardingOperation, collect_warnings},
        entity::{provision_forwarding::ForwardingModes, query::EntityQueryService},
    },
};
use axum::{
    Json,
    body::Bytes,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::collections::HashSet;

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(qparams): ValidatedQuery<PostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<QueryRequest>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_query;
    let context = context.merge(body_context);

    // Convert POST query body to EntityQueryParameters using From impl
    let mut params: EntityQueryParameters = request.into();
    // Merge fields from query string parameters
    params.options = qparams.options;
    params.local = qparams.local;
    params.pick = qparams.pick;
    params.omit = qparams.omit;
    params.dataset_id = qparams.dataset_id;
    params.format = qparams.format;
    params.join = qparams.join;
    params.join_level = qparams.join_level;
    params.geometry_property = qparams.geometry_property;
    apply_entity_query_extension_headers(&mut params, &protocol, &uri)?;
    let language_filter = protocol.language_filter;
    let incoming_via = protocol.incoming_via;
    params
        .validate()
        .map_err(|error| NgsiLdError::from(AndromedaError::from(error).with_instance(uri.clone())))?;

    let opts = params.parse_options();
    let join_mode = params.join.clone();
    let join_level = params.join_level.unwrap_or(1);
    let geometry_property = params.geometry_property.clone();

    let is_local = params.local.unwrap_or(false);
    let (local_entities, total_count) = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    // Distributed forwarding: query matching context sources unless local=true
    let (entities, warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_query(
                entity_type,
                entity_ids_ref,
                context_url,
                &context,
                ForwardingOperation::QueryBatch,
                incoming_via.as_ref(),
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Context source forwarding failed during batch query");
                Vec::new()
            });
        let warnings = collect_warnings(&forwarded);

        if forwarded.is_empty() {
            (local_entities, warnings)
        } else {
            let merged = EntityQueryService::merge_with_forwarded(local_entities, forwarded, None);
            (merged, warnings)
        }
    };

    // Handle join
    if matches!(join_mode.as_deref(), Some("inline") | Some("flat")) {
        let linked = service
            .resolve_linked_entities(&entities, join_level, opts.sys_attrs, &context, &HashSet::new())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if join_mode.as_deref() == Some("flat") {
            let mut all_values: Vec<Value> = Vec::new();
            for entity in &entities {
                all_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            for linked_entity in linked.values() {
                all_values.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .render_collection(all_values)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        } else {
            // Inline: renderer handles join + mode transformation
            let mut entity_values: Vec<Value> = Vec::new();
            for entity in &entities {
                entity_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_inline_linked_entities(&linked)
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .render_collection(entity_values)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        }
    }

    let entity_values = entities
        .iter()
        .map(|entity| {
            EntityRenderer::new(entity)
                .with_optional_language_filter(language_filter.as_ref())
                .with_mode(opts.render_mode)
                .render()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.with_instance(uri.clone()))?;
    let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
        .with_geometry_property(geometry_property)
        .render_collection(entity_values)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    Ok(response.into_response())
}

pub async fn batch_create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::bad_request_data("Request body must contain a non-empty array of entities")
            .with_instance(uri.clone())
            .into());
    }

    // Distributed forwarding: forward to matching context sources
    // Per spec 5.6.7: iterate CSRs, forward matching entities
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", |s| s.as_str()),
    });
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::CreateBatch,
            batch_path: "/entityOperations/create",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // Per spec 4.3.6.3: exclusive/redirect — don't create locally, return CS result
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::CREATED);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: create locally
    let batch = service.batch_create(&entities, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        let value = serde_json::to_value(&result.success).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response())
    } else {
        let value = serde_json::to_value(&result).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}

pub async fn batch_upsert(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<UpsertParams>,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::bad_request_data("Request body must contain a non-empty array of entities")
            .with_instance(uri.clone())
            .into());
    }

    let update_mode = params.options.as_deref() == Some("update");

    // Distributed forwarding: forward to matching context sources
    // Per spec 5.6.8: iterate CSRs, forward matching entities
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", |s| s.as_str()),
    });
    let qs = params.options.as_deref().map(|o| format!("options={o}"));
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::UpsertBatch,
            batch_path: "/entityOperations/upsert",
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: upsert locally
    let (batch, created_ids) = service
        .batch_upsert(&entities, &context, update_mode)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        if created_ids.is_empty() {
            Ok(StatusCode::NO_CONTENT.into_response())
        } else {
            let value = serde_json::to_value(&created_ids).map_err(|e| {
                AndromedaError::internal("Failed to serialize batch result")
                    .with_source(e)
                    .with_instance(uri.clone())
            })?;
            let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
            Ok(NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response())
        }
    } else {
        let value = serde_json::to_value(&result).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}

pub async fn batch_update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<UpdateParams>,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::bad_request_data("Request body must contain a non-empty array of entities")
            .with_instance(uri.clone())
            .into());
    }

    let no_overwrite = params.options.as_deref().is_some_and(|o| o.contains("noOverwrite"));

    // Distributed forwarding per spec 5.6.9
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", |s| s.as_str()),
    });
    let qs = params.options.as_deref().map(|o| format!("options={o}"));
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::UpdateBatch,
            batch_path: "/entityOperations/update",
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let batch = service
        .batch_update(&entities, &context, no_overwrite)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        let value = serde_json::to_value(&result).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}

pub async fn batch_merge(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::bad_request_data("Request body must contain a non-empty array of entities")
            .with_instance(uri.clone())
            .into());
    }

    // Distributed forwarding per spec 5.6.20
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", |s| s.as_str()),
    });
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::MergeBatch,
            batch_path: "/entityOperations/merge",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let batch = service.batch_merge(&entities, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        let value = serde_json::to_value(&result).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}

pub async fn batch_delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    body: Bytes,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let entity_ids: Vec<String> = serde_json::from_slice(&body)
        .map_err(|e| AndromedaError::invalid_request(format!("Request body must be a JSON array of entity IDs: {}", e)).with_instance(uri.clone()))?;

    if entity_ids.is_empty() {
        return Err(AndromedaError::bad_request_data("Request body must contain a non-empty array of entity IDs")
            .with_instance(uri.clone())
            .into());
    }

    // Distributed forwarding per spec 5.6.10
    let context: crate::jsonld::ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: None, // delete doesn't have entity type in payload
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteBatch,
            batch_path: "/entityOperations/delete",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        // Redirect: don't delete locally, return CS result
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response()); // clone: owned Value needed for JSON response
        }
        return Ok(status.into_response());
    }

    if modes.has_exclusive {
        // Exclusive: delete locally AND forwarded (already done)
        if let Err(e) = service.batch_delete(&entity_ids).await {
            tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
        }
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Inclusive or no match: delete locally
    let batch = service.batch_delete(&entity_ids).await.map_err(|e| e.with_instance(uri.clone()))?;
    let result: BatchOperationResult = batch.into();

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        let value = serde_json::to_value(&result).map_err(|e| {
            AndromedaError::internal("Failed to serialize batch result")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_batch(value)?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}
