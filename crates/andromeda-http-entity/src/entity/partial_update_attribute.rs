//! HTTP handling for the partial update attribute entity operation.

use crate::{
    entity::{
        AttributeWriteParams,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        IntoResponse,
        JsonLd,
        LocalOutcome,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        ProvisionResource,
        Response,
        StatusCode,
        TypedPath,
        ValidatedQuery,
        collect_proxied_attrs,
        local_entity_outcome,
        scoped_provision::ScopedProvision,
        update_result::{LocalAttributeWrite, attribute_write_response},
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
    written_payload_admission::admit_written_attribute,
};
use andromeda_entity::{write_kind::PartialUpdateAttribute, write_target::WriteTarget};
use andromeda_federation::{
    forwarded_content::{AttributeWrite, ForwardedContent},
    request::TriggeringRequest,
};
use andromeda_http_extraction::{
    context_target,
    path_segment::{AttrId, EntityId},
};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_model::attribute::{merge_member::MergeMember, written_attribute::WrittenAttribute};
use andromeda_ngsi_ld_request::query_narrowing::ExecutionScope;
use axum::http::Method;
use std::slice;

/// Partial Attribute Update (clause 5.6.4): 204 when the attribute was written
/// everywhere, 207 with the clause 5.2.18 `UpdateResult` otherwise.
pub(crate) async fn partial_update_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath((EntityId(id), AttrId(attr_id))): TypedPath<(EntityId, AttrId)>,
    ValidatedQuery(params): ValidatedQuery<AttributeWriteParams>,
    JsonLd { body, context }: JsonLd<WrittenAttribute>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment: MergeMember = body.admitted_attribute(&attr_id, &context).await?;
    let service = &state.entities;
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = forwarded_write_query([forwarded_type_parameter(uri.query())]);
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_selector: params.entity_type.as_ref(),
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
    let plan = ScopedProvision::plan(&state.forwarding, &target, &request, ExecutionScope::of_flag(params.local), &uri).await?;
    // Clause 5.6.4.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // attribute a proxied registration claims is still validated. The
    // registrations the plan sends the fragment to read the instance the
    // admission expanded.
    let admitted = admit_written_attribute::<PartialUpdateAttribute>(service, &attr_id, &fragment, &context)
        .await
        .with_entity_instance(&uri)?;
    let forwarded = plan
        .execute(
            &state.forwarding,
            &request,
            Some(ForwardedContent::Attribute {
                name: admitted.expanded_name(),
                instance: admitted.instance(),
                write: AttributeWrite::Fragment,
            }),
            &uri,
        )
        .await?;

    if forwarded.is_empty() {
        service.partial_update_attr(&target, entity, admitted).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.4.4: an attribute a proxied registration covers is updated at
    // the source, never here; an inclusive one is updated in both places.
    let name = admitted.expanded_name().clone(); // clone: the answer names the attribute the local update consumes
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        local_entity_outcome(service.partial_update_attr(&target, entity, admitted).await, &forwarded)
    } else {
        LocalOutcome::NothingRemained
    };
    let local = LocalAttributeWrite {
        outcome: &local,
        requested: slice::from_ref(&name),
        unwritten: Vec::new(),
    };
    attribute_write_response(local, &forwarded, &uri)
}
