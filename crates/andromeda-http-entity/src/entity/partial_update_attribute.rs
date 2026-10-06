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
        JsonLd,
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
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_model::attribute::{merge_member::MergeMember, written_attribute::WrittenAttribute};
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
    JsonLd { body, context }: JsonLd<WrittenAttribute>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment: MergeMember = body.read_attribute(&attr_id).admitted(&context)?;
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_compact: None,
        attribute_name: Some(&attr_id),
        method: Method::PATCH,
        context: &context,
        operation: ForwardingOperation::PartialUpdateAttribute,
        path_suffix: Some(&path_suffix),
        query_string: qs.as_deref(),
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = state.forwarding.plan_provision(&target, &request).await.with_entity_instance(&uri)?;
    // Clause 5.6.4.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // attribute a proxied registration claims is still validated. The
    // registrations the plan sends the fragment to read the instance the
    // admission expanded.
    let admitted = admit_written_attribute::<PartialUpdateAttribute>(service, &attr_id, &fragment, &context)
        .await
        .with_entity_instance(&uri)?;
    let forwarded = state
        .forwarding
        .execute_provision(
            plan,
            &request,
            Some(ForwardedContent::Attribute {
                name: admitted.expanded_name(),
                instance: admitted.instance(),
            }),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        service
            .partial_update_attr(&target, &id, dataset_id, admitted)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.4.4: an attribute a proxied registration covers is updated at
    // the source, never here; an inclusive one is updated in both places.
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        LocalOutcome::processed(service.partial_update_attr(&target, &id, dataset_id, admitted).await, classify_entity_error)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
