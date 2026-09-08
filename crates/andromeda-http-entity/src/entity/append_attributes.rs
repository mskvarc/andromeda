//! HTTP handling for the append attributes entity operation.

use crate::entity::{
    AppendParams,
    AttributeName,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    IntoResponse,
    Json,
    JsonLd,
    LocalOutcome,
    NgsiLdRequestEnvelope,
    OverwritePolicy,
    ProblemResponse,
    ProvisionResource,
    RequestOverwritePolicy,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    Value,
    classify_entity_error,
    collect_proxied_attrs,
    provision_response,
    reject_unknown_attribute_types,
    strip_attributes_from_value,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use axum::http::Method;

pub(crate) async fn append_attrs(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<AppendParams>,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let context = context.merge(body_context);
    reject_unknown_attribute_types(&fragment, &uri)?;
    let no_overwrite = params.options == Some(RequestOverwritePolicy::NoOverwrite);
    let overwrite = if no_overwrite {
        OverwritePolicy::Preserve
    } else {
        OverwritePolicy::Overwrite
    };

    // Forward to matching context sources
    let fragment_body = serde_json::to_vec(&fragment).with_entity_instance(&uri)?;
    let qs = if no_overwrite { "options=noOverwrite" } else { "" };

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: None,
                method: Method::POST,
                body: Some(&fragment_body),
                context: &context,
                operation: ForwardingOperation::AppendEntityAttributes,
                path_suffix: Some("/attrs"),
                query_string: if qs.is_empty() { None } else { Some(qs) },
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — append locally
        let (all_attrs, not_appended) = service
            .append_attrs(&target, &id, fragment, overwrite, &context)
            .await
            .with_entity_instance(&uri)?;

        if not_appended.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let updated: Vec<&AttributeName> = all_attrs.iter().filter(|attribute| !not_appended.contains(attribute)).collect();
        let not_updated_details: Vec<serde_json::Value> = not_appended
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Already exists" }))
            .collect();
        let body = serde_json::json!({ "updated": updated, "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    // Clause 5.6.3.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is appended here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut local_fragment = fragment;
    strip_attributes_from_value(&mut local_fragment, &proxied);
    let local = if local_fragment.as_object().is_some_and(serde_json::Map::is_empty) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::processed(
            service.append_attrs(&target, &id, local_fragment, overwrite, &context).await,
            classify_entity_error,
        )
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
