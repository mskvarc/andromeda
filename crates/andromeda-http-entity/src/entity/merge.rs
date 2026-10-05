//! HTTP handling for the merge entity operation (clause 5.6.17, bound by
//! clause 6.5.3.4 with the parameters of Table 6.5.3.4-1).

use crate::{
    entity::{
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
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
        classify_entity_error,
        collect_proxied_attrs,
        provision_response,
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::write_kind::MergeEntity;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use andromeda_jsonld::user_context_admission::admit_user_context;
use axum::http::Method;

pub(crate) async fn merge(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<MergeParams>,
    JsonLd { body, context }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let patch = body.admitted(&context)?;
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);
    let representation = params.representation().with_entity_instance(&uri)?;
    let options = MergeOptions {
        representation,
        lang: params.lang.as_ref(),
        observed_at: params.observed_at,
        type_selector: params.entity_type.as_ref(),
    };
    // Clause 5.6.17.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and a
    // merge left with nothing local is still validated.
    let admitted = admit_written_entity::<MergeEntity>(service, &patch, &context)
        .await
        .with_entity_instance(&uri)?;

    if is_local {
        service
            .merge(&target, &id, &patch, admitted, &context, options)
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
                entity_type_compact: None,
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
            Some(&patch),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .merge(&target, &id, &patch, admitted, &context, options)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.17.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is merged here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut patch = patch;
    let strip = patch.strip_attributes(&proxied);
    let local = match service.admit_local_remainder(admitted, strip, &patch, &context).await {
        Some(remainder) => LocalOutcome::processed(
            async { service.merge(&target, &id, &patch, remainder?, &context, options).await }.await,
            classify_entity_error,
        ),
        None => LocalOutcome::NothingRemained,
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
