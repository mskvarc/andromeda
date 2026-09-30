//! HTTP handling for Purge Entities (clause 5.6.21, bound to
//! `DELETE /entities` by clause 6.4.3.3).

use crate::entity::{
    BatchOperationResult,
    ContextSpec,
    EntityQuery,
    EntityResultExt,
    EntityTenantState,
    ForwardPurgeRequest,
    ForwardedPurgeResult,
    Identifier,
    IntoResponse,
    NgsiLdProtocolHeaders,
    NgsiLdRequestEnvelope,
    ProblemResponse,
    PurgeMode,
    Response,
    StatusCode,
    batch_operation_result,
    classify_entity_error,
    classify_forwarding_error,
    forwarded_write_error,
    multi_status,
    purge_entity_map::resolve_purge_map,
    reject_linked_retrieval,
    single_source_failure,
};
use andromeda_entity::side_effect::SideEffectPlacement;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{entity_selection::ConsideredEntities, projection::purge::DatasetPurgeScope};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::{Extension, http::Uri as RequestUri};
use std::slice;

/// The dataset scope of a purge, gated by the `purge_dataset_scope` extension.
///
/// `deleteAll` removes every instance; a single `datasetId` names one
/// (`purge datasetId` targets one instance, mirroring clause 5.6.5); absence of
/// both is the standard whole-attribute Purge. When a dataset scope is asked for
/// while the extension is off, clause 6.3.12's pattern answers `InvalidRequest`.
fn purge_dataset_scope(query: &EntityQueryParameters, enabled: EnabledExtensions, uri: &RequestUri) -> Result<DatasetPurgeScope, ProblemResponse> {
    let scope = if query.delete_all == Some(true) {
        DatasetPurgeScope::All
    } else if let Some(selector) = query.dataset_id.as_deref().and_then(<[_]>::first) {
        DatasetPurgeScope::Selector(selector.clone()) // clone: the compiled scope owns its selector
    } else {
        DatasetPurgeScope::Unfiltered
    };
    if !scope.is_unfiltered() && !enabled.purge_dataset_scope_enabled() {
        let report = ClassifiedReport::public(
            NgsiLdErrorType::InvalidRequest,
            "datasetId and deleteAll on Purge Entities require the purge_dataset_scope extension".to_string(),
        );
        return Err(report.with_instance(uri.clone()));
    }
    Ok(scope)
}

/// Fold the forwarded purges into the clause 5.2.16 result: every failed
/// registration contributes one clause 5.2.17 error per entity it is reported
/// against, naming the registration.
fn merge_forwarded_purges(result: &mut BatchOperationResult, forwarded: &[ForwardedPurgeResult]) {
    for entry in forwarded {
        for id in &entry.reported_ids {
            if let Some(error) = forwarded_write_error(id.as_uri().clone(), &entry.result.registration_id, &entry.result.outcome) {
                // clone: each error owns the identifier it is reported against
                result.errors.push(error);
            }
        }
    }
}

pub(crate) async fn delete_by_query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    Extension(enabled): Extension<EnabledExtensions>,
    EntityQuery(mut query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    if !target.permits_federation() {
        query.local = Some(true);
    }
    query.validate().with_entity_instance(&uri)?;
    // Clause 5.6.21.4 gives this operation no `join`: it renders nothing and
    // retrieves nothing, so a Linked Entity reference in `q` or in `pick`/`omit`
    // can never be satisfied and is rejected outright rather than measured
    // against a traversal that does not exist here.
    reject_linked_retrieval(query.pick.as_deref(), query.omit.as_deref(), query.q.as_ref()).with_entity_instance(&uri)?;

    let is_local = !target.permits_federation() || query.is_local();
    let context: ContextSpec = context.into_inner();

    // Table 6.4.3.3-2: "If present, the EntityMap supplied is used for
    // determining the set of Entities requested during the purge operation."
    // The narrowing runs before the query is compiled, so both the local phase
    // below and the identifiers it reports are the map's.
    let entity_map = resolve_purge_map(&state.entity_maps, &target, &mut query, protocol.entity_map.as_ref(), &uri).await?;

    // The dataset scope is the `purge_dataset_scope` extension: gated here (the
    // extractor has no config access), following clause 6.3.12's pattern of
    // answering an unsupported parameter with `InvalidRequest`. When the flag is
    // off, `datasetId`/`deleteAll` are refused and the standard whole-attribute
    // Purge (clause 5.6.21) is unchanged.
    let dataset = purge_dataset_scope(&query, enabled, &uri)?;

    // Determine purge mode from drop/keep parameters (clause 5.6.21).
    let purge_mode = if let Some(ref drop_attrs) = query.drop {
        PurgeMode::Drop {
            attributes: drop_attrs.clone(), // clone: the compiled mode owns its attribute list
            dataset,
        }
    } else if let Some(ref keep_attrs) = query.keep {
        PurgeMode::Keep {
            attributes: keep_attrs.clone(), // clone: the compiled mode owns its attribute list
            dataset,
        }
    } else {
        PurgeMode::DeleteEntities
    };

    let compiled = state.entity_query.compile_query_uncapped(&query, &context).await.with_entity_instance(&uri)?;
    let entity_ids: Vec<Identifier> = state.entity_query.query_ids(&target, &compiled).await.with_entity_instance(&uri)?;

    // Clause 5.6.21.4: the local phase deletes the matched entities, or only
    // the listed (or all but the listed) members of each.
    let mut result = match purge_mode {
        PurgeMode::Drop { .. } | PurgeMode::Keep { .. } => {
            state
                .entities
                .purge_attrs(&target, &entity_ids, &purge_mode, &context)
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
                .batch_delete(SideEffectPlacement::Inline(&target), &entity_ids)
                .await
                .with_entity_instance(&uri)?;
            batch_operation_result(batch.success, batch.errors, classify_entity_error)
        }
    };

    // Clause 5.5.14: an EntityMap "fixes the Entities to be considered for
    // subsequent requests based on it", so a purge left considering none -- its
    // identifiers and the map's do not intersect -- has no entity for any
    // context source to purge, and forwarding could only widen what the map
    // fixed.
    let considers_nothing = matches!(ConsideredEntities::of_request(query.id.as_deref()), ConsideredEntities::NoEntity);

    // Clause 5.6.21.4: unless the request is local, the purge is forwarded to
    // every matching registration that supports it.
    let forwarded = if is_local || considers_nothing {
        Vec::new()
    } else {
        state
            .forwarding
            .forward_purge(
                &target,
                ForwardPurgeRequest {
                    entity_type_compact: query.entity_type.as_ref(),
                    entity_ids: query.id.as_deref(),
                    local_ids: &entity_ids,
                    query_string: uri.query().unwrap_or(""),
                    entity_map: entity_map.in_use(),
                    context: &context,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                },
            )
            .await
            .with_entity_instance(&uri)?
    };
    let local_failed = !result.errors.is_empty();
    let remote_failed = forwarded.iter().any(|entry| entry.result.is_failure());
    if !local_failed && !remote_failed {
        return Ok(entity_map.attach_to(StatusCode::NO_CONTENT.into_response()));
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
    Ok(entity_map.attach_to(multi_status(result)))
}
