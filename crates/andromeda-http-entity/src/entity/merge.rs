//! HTTP handling for the merge entity operation (clause 5.6.17, bound by
//! clause 6.5.3.4 with the parameters of Table 6.5.3.4-1).

use crate::{
    entity::{
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        IntoResponse,
        JsonLd,
        LocalOutcome,
        MergeOptions,
        MergeParams,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        ProvisionResource,
        Response,
        StatusCode,
        TypedPath,
        ValidatedQuery,
        collect_proxied_attrs,
        local_entity_outcome,
        provision_response,
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::{write_kind::MergeEntity, write_target::WriteTarget};
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::{context_target, path_segment::EntityId};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_model::attribute::merge_member::MergeMember;
use andromeda_ngsi_ld_request::fragment_entity_id::admit_fragment_entity_id;
use axum::http::Method;

pub(crate) async fn merge(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    ValidatedQuery(params): ValidatedQuery<MergeParams>,
    JsonLd { body, context }: JsonLd<EntityPatch<MergeMember>>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let patch = body.admitted(&context).await?;
    admit_fragment_entity_id(patch.id.as_ref(), &id).with_entity_instance(&uri)?;
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);
    let representation = params.representation().with_entity_instance(&uri)?;
    let options = MergeOptions {
        representation,
        lang: params.lang.as_ref(),
        observed_at: params.observed_at,
        claimed: &[],
    };
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());
    // Clause 5.6.17.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and a
    // merge left with nothing local is still validated.
    let admitted = admit_written_entity::<MergeEntity>(service, &patch, &context)
        .await
        .with_entity_instance(&uri)?;

    if is_local {
        service
            .merge(&target, entity, &patch, admitted, &context, options)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_selector: params.entity_type.as_ref(),
                attribute_name: None,
                method: Method::PATCH,
                context: &context,
                operation: ForwardingOperation::MergeEntity,
                path_suffix: None,
                query_string: uri.query(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            Some(ForwardedContent::Entity(admitted.document())),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .merge(&target, entity, &patch, admitted, &context, options)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.17.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is merged here.
    let proxied = collect_proxied_attrs(&forwarded);
    let local = match admitted.local_remainder(&proxied) {
        Some(remainder) => local_entity_outcome(
            service
                .merge(&target, entity, &patch, remainder, &context, MergeOptions { claimed: &proxied, ..options })
                .await,
            &forwarded,
        ),
        None => LocalOutcome::NothingRemained,
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
