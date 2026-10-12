//! HTTP handling for the replace attribute entity operation.

use crate::{
    entity::{
        AttributeName,
        AttributeWriteParams,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
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
        update_result::{LocalAttributeWrite, attribute_write_response},
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
    written_payload_admission::admit_written_attribute,
};
use andromeda_entity::{write_kind::ReplaceAttribute, write_target::WriteTarget};
use andromeda_federation::{
    forwarded_content::{AttributeWrite, ForwardedContent},
    request::TriggeringRequest,
};
use andromeda_http_extraction::context_target;
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_model::attribute::{instance::AttributeValue, written_attribute::WrittenAttribute};
use axum::http::Method;
use std::slice;

pub(crate) async fn replace_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath((id, attr_id)): TypedPath<(Identifier, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<AttributeWriteParams>,
    JsonLd { body, context }: JsonLd<WrittenAttribute>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment: AttributeValue = body.read_attribute(&attr_id).admitted(&context)?;
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
    // attribute a proxied registration claims is still validated. The
    // registrations the plan sends the fragment to read the instance the
    // admission expanded.
    let admitted = admit_written_attribute::<ReplaceAttribute>(service, &attr_id, &fragment, &context)
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
                write: AttributeWrite::Replacement,
            }),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.replace_attr(&target, entity, admitted).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.19.4: an attribute a proxied registration covers is replaced
    // at the source, never here; an inclusive one is replaced in both places.
    let name = admitted.expanded_name().clone(); // clone: the answer names the attribute the local replacement consumes
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        local_entity_outcome(service.replace_attr(&target, entity, admitted).await, &forwarded)
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
