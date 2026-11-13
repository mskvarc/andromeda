//! HTTP handling for the append attributes entity operation.

use crate::{
    entity::{
        AppendParams,
        AttributeName,
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        JsonLd,
        LocalOutcome,
        NgsiLdRequestEnvelope,
        OverwritePolicy,
        ProblemResponse,
        ProvisionResource,
        RequestOverwritePolicy,
        Response,
        TypedPath,
        ValidatedQuery,
        collect_proxied_attrs,
        local_entity_outcome,
        scoped_provision::ScopedProvision,
        update_result::{LocalAttributeWrite, NotUpdatedReason, attribute_write_response},
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::{write_kind::AppendAttributes, write_target::WriteTarget};
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::{context_target, path_segment::EntityId};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_entity::admission::written_families::WrittenFamilies;
use andromeda_ngsi_ld_request::{fragment_entity_id::admit_fragment_entity_id, query_narrowing::ExecutionScope};
use axum::http::Method;

/// Append Entity Attributes (clause 5.6.3): 204 when every attribute was
/// written, 207 with the clause 5.2.18 `UpdateResult` otherwise.
pub(crate) async fn append_attrs(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    ValidatedQuery(params): ValidatedQuery<AppendParams>,
    JsonLd { body, context }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment = body.admitted(&context).await?;
    admit_fragment_entity_id(fragment.id.as_ref(), &id).with_entity_instance(&uri)?;
    let service = &state.entities;
    // The fragment is refused before the options are, so the options are
    // read here and their refusal raised after the admission.
    let requested_policy = params.overwrite_policy();
    let no_overwrite = matches!(requested_policy, Ok(RequestOverwritePolicy::NoOverwrite));
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());
    let qs = forwarded_write_query([
        no_overwrite.then(|| ("options", "noOverwrite".to_owned())),
        forwarded_type_parameter(uri.query()),
    ]);
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_selector: params.entity_type.as_ref(),
        attribute_name: None,
        method: Method::POST,
        context: &context,
        operation: ForwardingOperation::AppendEntityAttributes,
        path_suffix: Some("/attrs"),
        query_string: qs.as_deref(),
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = ScopedProvision::plan(&state.forwarding, &target, &request, ExecutionScope::of_flag(params.local), &uri).await?;
    // Clause 5.6.3.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // append left with nothing local is still validated. The registrations the
    // plan sends the fragment to read the document the admission expanded.
    let admitted = admit_written_entity::<AppendAttributes>(service, &fragment, &context, WrittenFamilies::Final)
        .await
        .with_entity_instance(&uri)?;
    let overwrite = match requested_policy.with_entity_instance(&uri)? {
        RequestOverwritePolicy::NoOverwrite => OverwritePolicy::Preserve,
        RequestOverwritePolicy::Overwrite => OverwritePolicy::Overwrite,
    };
    let sent_to_registrations = plan.reads_payload();
    let forwarded = plan
        .execute(&state.forwarding, &request, Some(ForwardedContent::Entity(admitted.document())), &uri)
        .await?;

    if forwarded.is_empty() {
        let (all_attrs, not_appended) = service.append_attrs(&target, entity, admitted, overwrite).await.with_entity_instance(&uri)?;
        let local = LocalAttributeWrite {
            outcome: &LocalOutcome::Processed(Ok(())),
            requested: &all_attrs,
            unwritten: NotUpdatedReason::AlreadyExists.of(&not_appended),
        };
        return attribute_write_response(local, &forwarded, &uri);
    }

    // Clause 5.6.3.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is appended here. A fragment no registration
    // read was taken over by none of them.
    let remainder = if sent_to_registrations {
        admitted.local_remainder(&collect_proxied_attrs(&forwarded))
    } else {
        Some(admitted)
    };
    let (local, requested, not_appended) = match remainder {
        None => (LocalOutcome::NothingRemained, Vec::new(), Vec::new()),
        Some(remainder) => {
            let attempted = remainder.document().attributes().keys().cloned().collect::<Vec<AttributeName>>(); // clone: a failed append is answered with the attributes it consumed
            match service.append_attrs(&target, entity, remainder, overwrite).await {
                Ok((requested, not_appended)) => (LocalOutcome::Processed(Ok(())), requested, not_appended),
                Err(error) => (local_entity_outcome(Err::<(), _>(error), &forwarded), attempted, Vec::new()),
            }
        }
    };
    let local = LocalAttributeWrite {
        outcome: &local,
        requested: &requested,
        unwritten: NotUpdatedReason::AlreadyExists.of(&not_appended),
    };
    attribute_write_response(local, &forwarded, &uri)
}
