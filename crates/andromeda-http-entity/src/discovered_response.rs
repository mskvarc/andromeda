//! The answer of a type or attribute discovery request (clauses 5.7.5 to
//! 5.7.10): the local answer, merged with what the matching context sources
//! answered unless the request is local, rendered under the request context.

use crate::{error::EntityResultExt, state::EntityRouteServices};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    discovery_result::{Discovered, collect_discovery_warnings},
    operation::ForwardingOperation,
    request::{DiscoveryTarget, ForwardDiscoveryRequest, TriggeringRequest},
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{accept::AcceptFormatNoGeo, api_root::ApiRoot, prefer::PreferHeader, request_envelope::NgsiLdRequestEnvelope};
use andromeda_http_response::{problem_response::ProblemResponse, rendered_response::RenderedResponse, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_render::{error::RenderError, response::ResponseRenderer};
use axum::response::{IntoResponse, Response};

/// One discovery request, its local answer already read.
pub(crate) struct DiscoveryRequest<'a> {
    /// The tenant's services.
    pub(crate) state: &'a EntityRouteServices,
    /// The branch the request reads.
    pub(crate) target: &'a ContextTarget,
    /// The Snapshot the response names, if the request read one.
    pub(crate) snapshot: Option<Identifier>,
    /// The negotiated representation.
    pub(crate) accept: AcceptFormatNoGeo,
    /// The request's URI, `Via` chain and headers.
    pub(crate) envelope: &'a NgsiLdRequestEnvelope,
    /// The request context every name is spelled in.
    pub(crate) context: &'a ContextSpec,
    /// The `apiRoot` a wrapper the response `Link` names is served under.
    pub(crate) api_root: &'a ApiRoot,
    /// Whether the request asked for local data alone (`local=true`).
    pub(crate) local_only: bool,
    /// What is discovered, which every source is asked the same way.
    pub(crate) discovered: DiscoveryTarget<'a>,
    /// The operation a source must advertise to be asked.
    pub(crate) operation: ForwardingOperation,
    /// The local answer.
    pub(crate) local: Discovered,
}

/// The discovery answer written under `renderer`: the single resources of
/// clauses 5.2.24, 5.2.26 and 5.2.27 as one object, and the `details=true`
/// records of clauses 5.2.25 and 5.2.28 as an array whose every item carries the
/// `@context` (clause 6.3.6).
///
/// # Errors
///
/// Returns the [`RenderError`] of an answer that could not be written.
fn written(renderer: ResponseRenderer<'_>, answer: &Discovered) -> Result<RenderedResponse, RenderError> {
    match answer {
        Discovered::Types(list) => RenderedResponse::resource(renderer.with_body(list)),
        Discovered::TypeDetails(types) => RenderedResponse::resource(renderer.with_collection(types)),
        Discovered::TypeInfo(info) => RenderedResponse::resource(renderer.with_body(info)),
        Discovered::Attributes(list) => RenderedResponse::resource(renderer.with_body(list)),
        Discovered::AttributeDetails(attributes) => RenderedResponse::resource(renderer.with_collection(attributes)),
        Discovered::AttributeInfo(info) => RenderedResponse::resource(renderer.with_body(info)),
    }
}

/// Answer a discovery request.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request context cannot be
/// processed, or when the answer cannot be rendered under the negotiated
/// media type.
pub(crate) async fn discovered_response(request: DiscoveryRequest<'_>) -> Result<Response, ProblemResponse> {
    let DiscoveryRequest {
        state,
        target,
        snapshot,
        accept,
        envelope,
        context,
        api_root,
        local_only,
        discovered,
        operation,
        local,
    } = request;
    let uri = &envelope.uri;
    let (answer, warnings) = if local_only || !target.permits_federation() {
        (local, Vec::new())
    } else {
        let forwarded = state
            .forwarding
            .forward_discovery_query(
                target,
                ForwardDiscoveryRequest {
                    target: discovered,
                    query_string: uri.query().unwrap_or(""),
                    context,
                    operation,
                    triggering: TriggeringRequest {
                        via: envelope.incoming_via.as_ref(),
                        headers: Some(&envelope.headers),
                    },
                },
            )
            .await;
        let mut warnings = collect_discovery_warnings(&forwarded);
        let (merged, merge_warnings) = state.forwarding.merge_discovered(local, forwarded, context).await.with_entity_instance(uri)?;
        warnings.extend(merge_warnings);
        (merged, warnings)
    };
    let configured = ResponseContextSource::new(state.entity_query.engine(), context, &state.context_hosting, api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_entity_instance(uri)?;
    let rendered = written(configured, &answer).with_entity_instance(uri)?;
    let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
    Ok(match snapshot {
        Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
        None => response.into_response(),
    })
}
