//! HTTP handling for temporal retrieve.

use crate::http::handler::temporal::{
    AcceptFormatNoGeo,
    AndromedaError,
    ContextSpec,
    ContextUrl,
    CsrModeFilter,
    ForwardingOperation,
    IntoResponse,
    NgsiLdError,
    NgsiLdProtocolHeaders,
    NgsiLdResponse,
    OriginalUri,
    PreferHeader,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    StatusCode,
    TemporalQueryParameters,
    TemporalSimplifyPayload,
    TemporalSimplifyRenderer,
    TenantState,
    TypedPath,
    Uri,
    ValidatedQuery,
    Value,
    collect_temporal_warnings,
    merge_temporal_retrieve_results,
    start_rendering,
};

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    protocol
        .extensions()
        .apply_to_temporal_query(&mut params)
        .map_err(|error| AndromedaError::from(error).with_instance(&uri))?;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve temporal entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_data = state
            .snapshots
            .get_snapshot_temporal_entity(&snapshot_id, &id)
            .await
            .with_instance(&uri)?
            .ok_or_else(|| NgsiLdError::from(AndromedaError::temporal_not_found(id.to_string()).with_instance(&uri)))?;

        let key_values = params.is_simplified();

        let data = if key_values {
            TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_data))
                .render()
                .with_instance(&uri)?
                .into_single()
                .with_instance(&uri)?
        } else {
            entity_data
        };
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(data))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

    // EntityMap scoping (spec 5.7.3.4): if NGSILD-EntityMap header is present,
    // only entities in the map are accessible. Return 404 if not in the map.
    if let Some(ref map_id) = entity_map_header {
        state.entity_maps.validate_entity_in_map(map_id.as_inner(), &id).await.with_instance(&uri)?;
    }

    let is_local = params.is_local();

    let local_result = service.get(&id, &params, &context).await;

    let (entity_value, content_range, warnings) = if is_local {
        let result = local_result.with_instance(&uri)?;
        (result.value, result.content_range, Vec::new())
    } else {
        let context_url = context.link_contexts().first();
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_temporal_retrieve(
                &id,
                query_string,
                context_url,
                ForwardingOperation::RetrieveTemporalEntity,
                incoming_via.as_ref(),
                CsrModeFilter::All,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        let local_value = match local_result {
            Ok(result) => Some((result.value, result.content_range)),
            Err(ref e) if e.is_not_found() => None,
            Err(e) => return Err(e.with_instance(&uri).into()),
        };

        let (local_entity, content_range) = match local_value {
            Some((value, cr)) => (Some(value), cr),
            None => (None, None),
        };

        let merged = merge_temporal_retrieve_results(local_entity, forwarded);
        match merged {
            Some(value) => (value, content_range, warnings),
            None => {
                return Err(NgsiLdError::from(AndromedaError::temporal_not_found(id.to_string()).with_instance(&uri)));
            }
        }
    };

    let key_values = params.is_simplified();

    let status = if content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let data: Value = if key_values {
        TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_value))
            .render()
            .with_instance(&uri)?
            .into_single()
            .with_instance(&uri)?
    } else {
        entity_value
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(data))
        .render()?;
    let mut builder = NgsiLdResponse::new(status, rendered).with_warnings(warnings);
    if let Some(map_header) = entity_map_header {
        builder = builder.with_entity_map_uri(map_header.into_inner());
    }
    if let Some(cr) = content_range {
        builder = builder.with_content_range(cr);
    }
    Ok(builder.into_response())
}
