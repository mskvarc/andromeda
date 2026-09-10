//! Asking the registered context sources for their part of a queried history
//! (clauses 5.7.4.4 and 6.3.18).

use crate::{
    error::single_source_declined,
    temporal::{
        ContextSpec,
        ContextualExpander,
        CsrModeFilter,
        ForwardTemporalQueryRequest,
        ForwardingOperation,
        ProblemResponse,
        ResultInstanceExt,
        TemporalQueryParameters,
        expand_q_expr,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use axum::http::Uri;
use tracing::warn;

/// How a temporal query reaches the context sources that hold the rest of the
/// history: the federation service that contacts them, the triggering request
/// every forwarded copy replays (clause 6.3.18), the `splitEntities` mode they
/// are asked under (clause 5.7.4.4), and the Entity Map that fixes which of
/// them are contacted (clause 5.5.14).
pub(crate) struct TemporalForwarding<'a> {
    /// The service that contacts every matching registration.
    pub(crate) forwarding: &'a FederationService,
    /// The triggering request every forwarded copy replays, so a forwarding
    /// loop is detected rather than followed.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// Whether an entity's attributes may be spread over several sources.
    pub(crate) split: SplitEntities,
    /// The Entity Map the query runs under.
    pub(crate) map_use: &'a EntityMapUse,
}

/// Ask every matching context source for its part of the history.
///
/// The context source filter selects registrations in its expanded form; the
/// raw expression travels on as the client wrote it, since clause 5.7.4.4
/// propagates it into discovery. A source that cannot be reached contributes a
/// clause 6.3.17 warning rather than failing the operation.
///
/// # Errors
///
/// The classified failure of expanding the context source filter, and the
/// Table 6.4.3.2-3 `NotImplemented` answer when the one source holding all the
/// data declined the requested representation.
pub(crate) async fn forward_temporal_history(
    sources: TemporalForwarding<'_>,
    engine: &JsonLdEngine,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    instance: &Uri,
) -> Result<Vec<ForwardedTemporalEntities>, ProblemResponse> {
    let TemporalForwarding {
        forwarding,
        triggering,
        split,
        map_use,
    } = sources;
    let csf_expr = match &params.csf {
        Some(expr) => {
            let prepared_context = engine.prepare_term_context(context);
            let expander = ContextualExpander::new(engine, &prepared_context);
            Some(expand_q_expr(expr.clone(), &expander).await.with_instance(instance)?) // clone: the raw filter stays in the query for forwarding
        }
        None => None,
    };
    let outcome = forwarding
        .forward_temporal_query(
            target,
            ForwardTemporalQueryRequest {
                client_query: params,
                split,
                context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering,
                csf_expr: csf_expr.as_ref(),
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(map_use),
                source_page: None,
            },
        )
        .await
        .unwrap_or_else(|error| {
            warn!(%error, "context source forwarding failed during the temporal query");
            ForwardReadOutcome::empty()
        });
    match outcome.sources.single_source_status {
        Some(status) => Err(single_source_declined(status, instance)),
        None => Ok(outcome.results),
    }
}
