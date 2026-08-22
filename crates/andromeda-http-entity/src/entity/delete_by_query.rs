//! HTTP handling for Purge Entities (clause 5.6.21, bound to
//! `DELETE /entities` by clause 6.4.3.3).

use crate::entity::{
    BatchOperationResult,
    ContextSpec,
    ContextUrl,
    EntityQuery,
    EntityResultExt,
    EntityTenantState,
    ForwardPurgeRequest,
    ForwardedPurgeResult,
    IncomingVia,
    IntoResponse,
    OriginalUri,
    ProblemResponse,
    PurgeMode,
    Response,
    StatusCode,
    Uri,
    batch_operation_result,
    classify_entity_error,
    classify_forwarding_error,
    forwarded_write_error,
    multi_status,
    reject_linked_retrieval,
    single_source_failure,
};
use andromeda_entity::side_effect::BatchSideEffectMode;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use axum::http::HeaderMap;
use std::slice;

/// Fold the forwarded purges into the clause 5.2.16 result: every failed
/// registration contributes one clause 5.2.17 error per entity it is reported
/// against, naming the registration.
fn merge_forwarded_purges(result: &mut BatchOperationResult, forwarded: &[ForwardedPurgeResult]) {
    for entry in forwarded {
        for id in &entry.reported_ids {
            if let Some(error) = forwarded_write_error(id.clone(), &entry.result.registration_id, &entry.result.outcome) {
                // clone: each error owns the identifier it is reported against
                result.errors.push(error);
            }
        }
    }
}

pub(crate) async fn delete_by_query(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    // Clause 5.6.21.4 gives this operation no `join`: it renders nothing and
    // retrieves nothing, so a Linked Entity reference in `q` or in `pick`/`omit`
    // can never be satisfied and is rejected outright rather than measured
    // against a traversal that does not exist here.
    reject_linked_retrieval(query.pick.as_deref(), query.omit.as_deref(), query.q.as_ref()).with_entity_instance(&uri)?;

    let is_local = query.is_local();
    let context: ContextSpec = context.into_inner();

    // Determine purge mode from drop/keep parameters (S5.6.21)
    let purge_mode = if let Some(ref drop_attrs) = query.drop {
        PurgeMode::Drop(drop_attrs.clone())
    } else if let Some(ref keep_attrs) = query.keep {
        PurgeMode::Keep(keep_attrs.clone())
    } else {
        PurgeMode::DeleteEntities
    };

    let compiled = state.entity_query.compile_query_uncapped(&query, &context).await.with_entity_instance(&uri)?;
    let entity_ids: Vec<Uri> = state.entity_query.query_ids(&compiled).await.with_entity_instance(&uri)?;

    // Clause 5.6.21.4: the local phase deletes the matched entities, or only
    // the listed (or all but the listed) members of each.
    let mut result = match purge_mode {
        PurgeMode::Drop(_) | PurgeMode::Keep(_) => {
            state
                .entities
                .purge_attrs(&entity_ids, &purge_mode, &context)
                .await
                .with_entity_instance(&uri)?;
            BatchOperationResult {
                success: entity_ids.clone(), // clone: the result owns the identifiers it reports
                errors: Vec::new(),
            }
        }
        PurgeMode::DeleteEntities if entity_ids.is_empty() => BatchOperationResult {
            success: Vec::new(),
            errors: Vec::new(),
        },
        PurgeMode::DeleteEntities => {
            let batch = state
                .entities
                .batch_delete(&entity_ids, BatchSideEffectMode::Inline)
                .await
                .with_entity_instance(&uri)?;
            batch_operation_result(batch.success, batch.errors, classify_entity_error)
        }
    };

    // Clause 5.6.21.4: unless the request is local, the purge is forwarded to
    // every matching registration that supports it.
    let forwarded = if is_local {
        Vec::new()
    } else {
        state
            .forwarding
            .forward_purge(ForwardPurgeRequest {
                entity_type_compact: query.entity_type.as_ref(),
                entity_ids: query.id.as_deref(),
                local_ids: &entity_ids,
                query_string: uri.query().unwrap_or(""),
                context: &context,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            })
            .await
            .with_entity_instance(&uri)?
    };
    let local_failed = !result.errors.is_empty();
    let remote_failed = forwarded.iter().any(|entry| entry.result.is_failure());
    if !local_failed && !remote_failed {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    // Clause 6.3.17: a purge that did nothing here and failed at its single
    // proxied source carries that source's error; anything else is partial.
    if entity_ids.is_empty()
        && let [single] = forwarded.as_slice()
        && let Some(error) = single_source_failure(slice::from_ref(&single.result))
    {
        return Err(classify_forwarding_error(&error).with_instance(uri));
    }
    merge_forwarded_purges(&mut result, &forwarded);
    Ok(multi_status(result))
}
