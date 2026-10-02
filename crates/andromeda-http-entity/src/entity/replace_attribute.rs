//! HTTP handling for the replace attribute entity operation.

use crate::{
    entity::{
        AttributeName,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
        IntoResponse,
        JsonLdDocument,
        LocalOutcome,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        ProvisionResource,
        ReplaceAttrParams,
        Response,
        StatusCode,
        TypedPath,
        ValidatedQuery,
        classify_entity_error,
        collect_proxied_attrs,
        provision_response,
        selected_dataset_uri,
    },
    written_payload_admission::admit_written_attribute,
};
use andromeda_entity::write_kind::ReplaceAttribute;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use axum::http::Method;

pub(crate) async fn replace_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath((id, attr_id)): TypedPath<(Identifier, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<ReplaceAttrParams>,
    JsonLdDocument { document: fragment, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_compact: None,
        attribute_name: Some(&attr_id),
        method: Method::PUT,
        context: &context,
        operation: ForwardingOperation::ReplaceEntityAttribute,
        path_suffix: Some(&path_suffix),
        query_string: qs.as_deref(),
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = state.forwarding.plan_provision(&target, &request).await.with_entity_instance(&uri)?;
    // Clause 5.6.19.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // attribute a proxied registration claims is still validated. Only a
    // registration the plan sends the fragment to reads it again, so it is
    // serialised for them before it moves into the admission.
    let payload = plan.payload(&fragment).with_entity_instance(&uri)?;
    let admitted = admit_written_attribute::<ReplaceAttribute>(service, &attr_id, fragment, &context)
        .await
        .with_entity_instance(&uri)?;
    let forwarded = state
        .forwarding
        .execute_provision(plan, &request, payload.as_deref())
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.replace_attr(&target, &id, dataset_id, admitted).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.19.4: an attribute a proxied registration covers is replaced
    // at the source, never here; an inclusive one is replaced in both places.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::processed(service.replace_attr(&target, &id, dataset_id, admitted).await, classify_entity_error)
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
