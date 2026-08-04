use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    core::error::{AndromedaError, ResultInstanceExt},
    domain::{entity::EntityTypeValue, expanded::OverwritePolicy},
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
            snapshot::SnapshotHeader,
            via::IncomingVia,
        },
        header::preference_applied::PreferenceApplied,
        model::{preference::RespondMode, query_request::QueryRequest},
        parameter::{
            entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
            protocol::apply_entity_query_extension_headers,
            query::entity::EntityQueryParameters,
        },
        response::{batch::NgsiLdBatchResponse, collection::NgsiLdCollectionResponse, sonic_json::SonicJson},
    },
    renderer::{
        Renderer,
        entity::EntityRenderer,
        response::{ResponsePayload, ResponseRenderer},
    },
    service::{
        csource_forwarding::{CsrModeFilter, ForwardBatchProvisionRequest, ForwardQueryRequest, ForwardingOperation, collect_warnings},
        entity::{BatchSideEffectMode, provision_forwarding::ForwardingModes, query::EntityQueryService},
    },
};
use axum::{
    body::Bytes,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use serde_json::Value;
use std::collections::HashSet;

fn apply_preference_applied(mut response: Response, mode: BatchSideEffectMode) -> Response {
    if matches!(mode, BatchSideEffectMode::Enqueue) {
        response.headers_mut().typed_insert(PreferenceApplied::respond_async());
    }
    response
}

/// Handles POST query requests for batch entity retrieval (NGSI-LD 5.7.2).
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    SnapshotHeader(snapshot_id): SnapshotHeader,
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

    // ETSI GS CIM 009 clause 5.5.15: when NGSILD-Snapshot is set, serve frozen
    // data and skip both the live pipeline and CSR forwarding.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let limit = i64::from(params.limit.unwrap_or(1000));
        let offset = i64::from(params.offset.unwrap_or(0));
        let (entities, _total) = state
            .snapshots
            .query_snapshot_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Collection(entities))
            .render()?;
        return Ok(NgsiLdCollectionResponse::ok(rendered).into_response());
    }
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
    params.validate().with_instance(&uri).map_err(NgsiLdError::from)?;

    let opts = params.parse_options();
    let join_mode = params.join.clone();
    let join_level = params.join_level.unwrap_or(1);
    let geometry_property = params.geometry_property.clone();

    let is_local = params.local.unwrap_or(false);
    let (local_entities, total_count) = service.query(&params, &context).await.with_instance(&uri)?;

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
            .forward_query(ForwardQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                context_url,
                context: &context,
                operation: ForwardingOperation::QueryBatch,
                incoming_via: incoming_via.as_ref(),
                csf_expr: None, // csf not supported on batch query endpoint
                mode_filter: CsrModeFilter::All,
            })
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
            .with_instance(&uri)?;

        if join_mode.as_deref() == Some("flat") {
            let mut all_values: Vec<Value> = Vec::new();
            for entity in &entities {
                all_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            for linked_entity in linked.values() {
                all_values.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .with_payload(ResponsePayload::Collection(all_values))
                .render()?;
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
                        .with_instance(&uri)?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .with_payload(ResponsePayload::Collection(entity_values))
                .render()?;
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
        .with_instance(&uri)?;
    let rendered = ResponseRenderer::new(accept.format(), context, protocol.prefer)
        .with_geometry_property(geometry_property)
        .with_payload(ResponsePayload::Collection(entity_values))
        .render()?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    Ok(response.into_response())
}

/// Handles batch entity creation requests (NGSI-LD 5.6.7).
pub async fn batch_create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entities")
            .with_instance(&uri)
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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // Per spec 4.3.6.3: exclusive/redirect — don't create locally, return CS result
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::CREATED);
        if let Some(ref resp_body) = first.body {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: create locally
    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entities.len());
    let batch = service.batch_create(&entities, &context, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        let value = serde_json::to_value(&result.success)
            .map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
            mode,
        ))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}

/// Handles batch entity upsert requests (NGSI-LD 5.6.8).
pub async fn batch_upsert(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    ValidatedQuery(params): ValidatedQuery<UpsertParams>,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entities")
            .with_instance(&uri)
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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: upsert locally
    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entities.len());
    let (batch, created_ids) = service.batch_upsert(&entities, &context, update_mode, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        if created_ids.is_empty() {
            Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
        } else {
            let value = serde_json::to_value(&created_ids)
                .map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
            let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
                .with_payload(ResponsePayload::Batch(value))
                .render()?;
            Ok(apply_preference_applied(
                NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
                mode,
            ))
        }
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}

/// Handles batch entity update requests (NGSI-LD 5.6.9).
pub async fn batch_update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    ValidatedQuery(params): ValidatedQuery<UpdateParams>,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entities")
            .with_instance(&uri)
            .into());
    }

    let no_overwrite = params.options.as_deref().is_some_and(|o| o.contains("noOverwrite"));
    let overwrite = if no_overwrite {
        OverwritePolicy::Preserve
    } else {
        OverwritePolicy::Overwrite
    };

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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entities.len());
    let batch = service.batch_update(&entities, &context, overwrite, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}

/// Handles batch entity merge requests (NGSI-LD 5.6.20).
pub async fn batch_merge(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entities")
            .with_instance(&uri)
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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entities.len());
    let batch = service.batch_merge(&entities, &context, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = batch.into();
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}

/// Handles batch entity deletion requests (NGSI-LD 5.6.10).
pub async fn batch_delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    body: Bytes,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let entity_ids: Vec<String> = serde_json::from_slice(&body)
        .map_err(|e| AndromedaError::malformed_request(format!("Request body must be a JSON array of entity IDs: {}", e)).with_instance(&uri))?;

    if entity_ids.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entity IDs")
            .with_instance(&uri)
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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        // Redirect: don't delete locally, return CS result
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, SonicJson(resp_body.clone())).into_response()); // clone: owned Value needed for JSON response
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entity_ids.len());

    if modes.has_exclusive {
        // Exclusive: delete locally AND forwarded (already done)
        if let Err(e) = service.batch_delete(&entity_ids, mode).await {
            tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
        }
        return Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode));
    }

    // Inclusive or no match: delete locally
    let batch = service.batch_delete(&entity_ids, mode).await.with_instance(&uri)?;
    let result: BatchOperationResult = batch.into();

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}
