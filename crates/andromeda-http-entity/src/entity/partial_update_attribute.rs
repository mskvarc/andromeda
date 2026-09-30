//! HTTP handling for the partial update attribute entity operation.

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
        PartialUpdateAttrParams,
        ProblemResponse,
        ProvisionResource,
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
use andromeda_entity::write_kind::PartialUpdateAttribute;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use axum::http::Method;

pub(crate) async fn partial_update_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath((id, attr_id)): TypedPath<(Identifier, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<PartialUpdateAttrParams>,
    JsonLdDocument { document: fragment, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    // Clause 5.6.4.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // attribute a proxied registration claims is still validated.
    // clone: the admission expands its own copy, while the forwarding phase
    // still sends the compact fragment
    let admitted = admit_written_attribute::<PartialUpdateAttribute>(service, &attr_id, fragment.clone(), &context)
        .await
        .with_entity_instance(&uri)?;

    // Forward to matching context sources
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: Some(&attr_id),
                method: Method::PATCH,
                body: Some(&fragment),
                context: &context,
                operation: ForwardingOperation::PartialUpdateAttribute,
                path_suffix: Some(&path_suffix),
                query_string: qs.as_deref(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        service
            .partial_update_attr(&target, &id, dataset_id, admitted, &context)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.4.4: an attribute a proxied registration covers is updated at
    // the source, never here; an inclusive one is updated in both places.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::processed(
            service.partial_update_attr(&target, &id, dataset_id, admitted, &context).await,
            classify_entity_error,
        )
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
