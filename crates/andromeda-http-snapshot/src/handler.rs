//! Handlers for the `/snapshots` endpoints.

use crate::{
    error::{SnapshotResultExt, internal_problem, project_purge_failure},
    state::SnapshotTenantState,
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    jsonld::JsonLd,
    query::ValidatedQuery,
    snapshot_purge::SnapshotPurgeQ,
    tenant_state,
    typed_path::TypedPath,
};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    headers::with_location,
    internal_failure_summary::InternalFailureSummary,
    problem_response::ProblemResponse,
    render::{RenderingPreferences, start_rendering_core},
    single::NgsiLdResponse,
};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{snapshot_clone::SnapshotCloneInput, snapshot_input::SnapshotInput, snapshot_patch::SnapshotPatch};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::empty::EmptyParams;
use andromeda_snapshot::service::purge::PurgeOutcome;
use axum::{
    Json,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/snapshots -- Create a new snapshot.
///
/// The body is a JSON-LD document (clause 6.3.5): under `application/ld+json`
/// it carries its own `@context`, which the extractor lifts off before the
/// Snapshot members are read. The snapshot stores its queries as written,
/// beside the context they are later run under, which is admitted first
/// (clause 5.5.7).
pub async fn create(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    JsonLd { value: input, context }: JsonLd<SnapshotInput>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(&state.engine, context).await.with_snapshot_instance(&uri)?;
    let snapshot = state.snapshots.create(input, context).await.with_snapshot_instance(&uri)?;
    state.snapshots.prepare_now();

    let location = format!("/ngsi-ld/v1/snapshots/{}", snapshot.id);

    Ok(with_location(StatusCode::CREATED.into_response(), location))
}

/// GET /ngsi-ld/v1/snapshots/{snapshot_id} -- Retrieve snapshot status.
pub async fn get(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    TypedPath(snapshot_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    let snapshot = state.snapshots.get(&snapshot_id).await.with_snapshot_instance(&uri)?;

    let body = serde_json::to_value(&snapshot).map_err(|error| internal_problem(&error, InternalFailureSummary::RESPONSE_ENCODING, &uri))?;
    let rendered = start_rendering_core(accept.media_type(), RenderingPreferences::default())
        .with_payload(ResponsePayload::Single(body))
        .render()
        .map_err(|error| internal_problem(&error, InternalFailureSummary::new("failed to render the snapshot"), &uri))?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// PATCH /ngsi-ld/v1/snapshots/{snapshot_id} -- Update snapshot status.
pub async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    TypedPath(snapshot_id): TypedPath<Identifier>,
    JsonLd { value: patch, context }: JsonLd<SnapshotPatch>,
) -> Result<Response, ProblemResponse> {
    // A Snapshot patch names no term a context could define.
    context.unused();
    let updated = state.snapshots.update(&snapshot_id, patch).await.with_snapshot_instance(&uri)?;

    let body = serde_json::to_value(&updated).map_err(|error| internal_problem(&error, InternalFailureSummary::RESPONSE_ENCODING, &uri))?;
    let rendered = start_rendering_core(accept.media_type(), RenderingPreferences::default())
        .with_payload(ResponsePayload::Single(body))
        .render()
        .map_err(|error| internal_problem(&error, InternalFailureSummary::new("failed to render the snapshot"), &uri))?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// DELETE /ngsi-ld/v1/snapshots/{snapshot_id} -- Delete a snapshot.
pub async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(snapshot_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    state.snapshots.delete(&snapshot_id).await.with_snapshot_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/snapshots -- Purge snapshots selected by a `q` filter.
///
/// Implements ETSI GS CIM 009 v1.9.1 clauses 5.16.7 and 6.36.3.2:
///
/// - Missing or malformed `q` -> 400 `BadRequestData` (handled by the
///   [`SnapshotPurgeQ`] extractor).
/// - No matches -> 404 `ResourceNotFound`.
/// - All matches deleted -> 204 No Content.
/// - At least one deletion failed -> 207 Multi-Status with a
///   [`BatchOperationResult`] body.
pub async fn purge(
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    SnapshotPurgeQ(filter): SnapshotPurgeQ,
) -> Result<Response, ProblemResponse> {
    let outcome = state.snapshots.purge_by_query(&filter).await.with_snapshot_instance(&uri)?;

    match outcome {
        PurgeOutcome::NoMatch => Err(ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, "no snapshots matched the purge query").with_instance(uri)),
        PurgeOutcome::FullSuccess => Ok(StatusCode::NO_CONTENT.into_response()),
        PurgeOutcome::AllFailed { failures } => {
            let body = BatchOperationResult {
                success: Vec::new(),
                errors: failures.into_iter().map(project_purge_failure).collect(),
            };
            Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
        }
        PurgeOutcome::Partial { successes, failures } => {
            let body = BatchOperationResult {
                success: successes,
                errors: failures.into_iter().map(project_purge_failure).collect(),
            };
            Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
        }
    }
}

/// POST /ngsi-ld/v1/snapshots/{snapshot_id}/clone -- Clone a snapshot.
pub async fn clone(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    tenant_state::ExistingTenantState(state): SnapshotTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(snapshot_id): TypedPath<Identifier>,
    JsonLd { value: input, context }: JsonLd<SnapshotCloneInput>,
) -> Result<Response, ProblemResponse> {
    // A clone names no term a context could define; its queries keep the
    // context the source Snapshot was created under.
    context.unused();
    let cloned = state.snapshots.clone_snapshot(&snapshot_id, input).await.with_snapshot_instance(&uri)?;
    state.snapshots.prepare_now();

    let location = format!("/ngsi-ld/v1/snapshots/{}", cloned.id);
    Ok(with_location(StatusCode::CREATED.into_response(), location))
}
