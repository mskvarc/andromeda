//! HTTP handling for the update attributes entity operation.

use crate::{
    entity::{
        AttributeName,
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        IncomingVia,
        JsonLd,
        LocalOutcome,
        OriginalUri,
        ProblemResponse,
        ProvisionResource,
        Response,
        TypedPath,
        UpdateAttributesParams,
        ValidatedQuery,
        collect_proxied_attrs,
        local_entity_outcome,
        scoped_provision::ScopedProvision,
        update_result::{LocalAttributeWrite, NotUpdatedReason, attribute_write_response},
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::{write_kind::UpdateAttributes, write_target::WriteTarget};
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::{context_target, path_segment::EntityId};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_request::{fragment_entity_id::admit_fragment_entity_id, query_narrowing::ExecutionScope};
use axum::http::{HeaderMap, Method};

pub(crate) async fn update_attrs(
    // Clause 6.3.20: the operation defines `type` alone (Table 6.6.3.2-1), so
    // any other parameter is an `InvalidRequest`; it is read first, before the
    // tenant is resolved.
    ValidatedQuery(params): ValidatedQuery<UpdateAttributesParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd { body, context }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment = body.admitted(&context).await?;
    admit_fragment_entity_id(fragment.id.as_ref(), &id).with_entity_instance(&uri)?;
    let service = &state.entities;
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());
    let qs = forwarded_write_query([forwarded_type_parameter(uri.query())]);
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_selector: params.entity_type.as_ref(),
        attribute_name: None,
        method: Method::PATCH,
        context: &context,
        operation: ForwardingOperation::UpdateEntityAttributes,
        path_suffix: Some("/attrs"),
        query_string: qs.as_deref(),
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = ScopedProvision::plan(&state.forwarding, &target, &request, ExecutionScope::of_flag(params.local), &uri).await?;
    // Clause 5.6.2.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // update left with nothing local is still validated. The registrations the
    // plan sends the fragment to read the document the admission expanded.
    let admitted = admit_written_entity::<UpdateAttributes>(service, &fragment, &context)
        .await
        .with_entity_instance(&uri)?;
    let sent_to_registrations = plan.reads_payload();
    let forwarded = plan
        .execute(&state.forwarding, &request, Some(ForwardedContent::Entity(admitted.document())), &uri)
        .await?;

    if forwarded.is_empty() {
        let (requested, not_updated) = service.update_attrs(&target, entity, admitted).await.with_entity_instance(&uri)?;
        let local = LocalAttributeWrite {
            outcome: &LocalOutcome::Processed(Ok(())),
            requested: &requested,
            unwritten: NotUpdatedReason::AttributeNotFound.of(&not_updated),
        };
        return attribute_write_response(local, &forwarded, &uri);
    }

    // Clause 5.6.2.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is updated here. A fragment no registration
    // read was taken over by none of them.
    let remainder = if sent_to_registrations {
        admitted.local_remainder(&collect_proxied_attrs(&forwarded))
    } else {
        Some(admitted)
    };
    let (local, requested, not_updated) = match remainder {
        None => (LocalOutcome::NothingRemained, Vec::new(), Vec::new()),
        Some(remainder) => {
            let attempted = remainder.document().attributes().keys().cloned().collect::<Vec<AttributeName>>(); // clone: a failed update is answered with the attributes it consumed
            match service.update_attrs(&target, entity, remainder).await {
                Ok((requested, not_updated)) => (LocalOutcome::Processed(Ok(())), requested, not_updated),
                Err(error) => (local_entity_outcome(Err::<(), _>(error), &forwarded), attempted, Vec::new()),
            }
        }
    };
    let local = LocalAttributeWrite {
        outcome: &local,
        requested: &requested,
        unwritten: NotUpdatedReason::AttributeNotFound.of(&not_updated),
    };
    attribute_write_response(local, &forwarded, &uri)
}
