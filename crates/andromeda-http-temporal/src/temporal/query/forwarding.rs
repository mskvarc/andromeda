//! Asking the registered context sources for their part of a queried history
//! (clauses 5.7.4.4 and 6.3.18).

use crate::temporal::{
    ContextSpec,
    ContextualExpander,
    CsrModeFilter,
    ForwardTemporalQueryRequest,
    ForwardingOperation,
    JsonKeyAttributes,
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    expand_q_expr,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_response::declined_representation::single_source_declined;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use axum::http::Uri;
use tracing::warn;

/// How far a temporal query reaches (clause 5.5.13 `local`).
///
/// Clause 5.5.13 lets a request be "limited to the local scope", and a Snapshot
/// branch has no distributed reading at all, so both answers are decided here
/// once and the forwarding reads that answer rather than a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryScope {
    /// The query stays inside this broker; no context source is contacted.
    Local,
    /// The query is spread over every matching registration.
    Distributed,
}

impl QueryScope {
    /// How far `params` reaches against `target`.
    pub(crate) fn of(target: &ContextTarget, params: &TemporalQueryParameters) -> QueryScope {
        if target.permits_federation() && !params.is_local() {
            QueryScope::Distributed
        } else {
            QueryScope::Local
        }
    }

    /// Whether an entity's attributes may be met spread over several sources.
    ///
    /// Clause 5.7.4.4: only a distributed read can meet a split entity, so a
    /// local one always sees complete entities and defers no filter.
    pub(crate) const fn splitting(self, requested: SplitEntities) -> SplitEntities {
        match self {
            QueryScope::Local => SplitEntities::Complete,
            QueryScope::Distributed => requested,
        }
    }
}

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

/// Ask every matching context source for its part of the history, unless the
/// query stays inside this broker.
///
/// The context source filter selects registrations in its expanded form; the
/// raw expression travels on as the client wrote it, since clause 5.7.4.4
/// propagates it into discovery. A source that cannot be reached contributes a
/// clause 6.3.17 warning rather than failing the operation. A clause 5.5.13
/// local query contacts nothing and answers with no contribution at all, which
/// is what the absent result means.
///
/// # Errors
///
/// The classified failure of expanding the context source filter, and the
/// Table 6.4.3.2-3 `NotImplemented` answer when the one source holding all the
/// data declined the requested representation.
pub(crate) async fn forward_temporal_history(
    sources: TemporalForwarding<'_>,
    scope: QueryScope,
    engine: &JsonLdEngine,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    instance: &Uri,
) -> Result<Option<Vec<ForwardedTemporalEntities>>, ProblemResponse> {
    let TemporalForwarding {
        forwarding,
        triggering,
        split,
        map_use,
    } = sources;
    if scope == QueryScope::Local {
        return Ok(None);
    }
    // No `jsonKeys` declaration applies to `csf`: Table 5.2.12-1 scopes that
    // member to Entity attribute names, while `csf` addresses the Context
    // Source Properties of a registration (clause 4.10).
    let csf_expr = match &params.csf {
        Some(expr) => {
            let prepared_context = engine.prepare_term_context(context);
            let expander = ContextualExpander::new(engine, &prepared_context);
            Some(
                expand_q_expr(expr.clone(), &JsonKeyAttributes::default(), &expander)
                    .await
                    .with_instance(instance)?,
            ) // clone: the raw filter stays in the query for forwarding
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
        None => Ok(Some(outcome.results)),
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::forwarding::QueryScope;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_federation::config::SplitEntities;
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

    /// The tenant's ordinary live branch, which a distributed read may leave.
    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    /// A temporal query confined to this broker (clause 5.5.13 `local=true`).
    fn local_query() -> TemporalQueryParameters {
        TemporalQueryParameters {
            local: Some(true),
            ..TemporalQueryParameters::default()
        }
    }

    /// Clause 5.5.13: a request limited to the local scope contacts no context
    /// source, whatever registrations would otherwise match.
    #[test]
    fn a_local_query_never_leaves_this_broker() {
        pretty_assertions::assert_eq!(QueryScope::of(&live_target(), &local_query()), QueryScope::Local);
    }

    /// A query that did not ask for the local scope reaches every matching
    /// registration (clause 5.7.4.4).
    #[test]
    fn a_query_that_did_not_ask_for_the_local_scope_is_distributed() {
        pretty_assertions::assert_eq!(QueryScope::of(&live_target(), &TemporalQueryParameters::default()), QueryScope::Distributed);
    }

    /// Clause 5.7.4.4: only a distributed read can meet an entity whose
    /// attributes are spread over several sources, so a local one always sees
    /// complete entities however the request asked.
    #[test]
    fn a_local_query_always_sees_complete_entities() {
        pretty_assertions::assert_eq!(QueryScope::Local.splitting(SplitEntities::Split), SplitEntities::Complete);
        pretty_assertions::assert_eq!(QueryScope::Distributed.splitting(SplitEntities::Split), SplitEntities::Split);
        pretty_assertions::assert_eq!(QueryScope::Distributed.splitting(SplitEntities::Complete), SplitEntities::Complete);
    }
}
