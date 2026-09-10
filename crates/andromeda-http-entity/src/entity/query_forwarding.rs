//! The distributed part of a query: the two clause 5.7.2.4 forwarding phases
//! and what they bring back.

use crate::entity::{
    ContextSpec,
    ContextualExpander,
    CsrModeFilter,
    EntityResultExt,
    ForwardQueryRequest,
    ForwardingOperation,
    JsonKeyAttributes,
    ProblemResponse,
    SourceContribution,
    collect_warnings,
    contributions_of,
    expand_q_expr,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    forwarded_query::ClientQuery,
    outcome::{ForwardReadOutcome, ForwardedEntities},
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::declined_representation::single_source_declined;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::filter::expression::Expr;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::Uri as HttpUri;
use tracing::warn;

/// What the two forwarding phases of a query brought back.
pub(crate) struct ForwardedQueryResults {
    /// The answers of the exclusive, redirect and inclusive sources.
    pub(crate) primary: Vec<ForwardedEntities>,
    /// The answers of the auxiliary sources (clause 4.3.6.2).
    pub(crate) auxiliary: Vec<ForwardedEntities>,
    /// The clause 6.3.17 warnings of both phases.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    pub(crate) contributions: Vec<SourceContribution>,
}

impl ForwardedQueryResults {
    /// Whether any source returned an entity, which is what makes the local
    /// page an incomplete view of the result (clause 5.5.9).
    pub(crate) fn holds_remote_entities(&self) -> bool {
        self.primary.iter().chain(&self.auxiliary).any(|result| !result.entities.is_empty())
    }

    /// Fold what the sources returned onto the entities the broker holds
    /// itself: the primary phase's answers first, then the auxiliary ones,
    /// which only supplement what is present already (clause 4.3.6.2).
    pub(crate) fn merge_onto(self, local_entities: Vec<Entity>, attrs: Option<&[AttributeName]>) -> MergedEntities {
        let merged = if self.primary.is_empty() {
            local_entities
        } else {
            EntityQueryService::merge_with_forwarded(local_entities, self.primary, attrs)
        };
        let merged = if self.auxiliary.is_empty() {
            merged
        } else {
            EntityQueryService::merge_with_forwarded(merged, self.auxiliary, attrs)
        };
        MergedEntities {
            entities: merged,
            warnings: self.warnings,
            contributions: self.contributions,
        }
    }
}

/// The entities a query answers with before its page is cut, together with
/// what the forwarding reported about the sources that helped.
pub(crate) struct MergedEntities {
    /// The local and forwarded entities, merged.
    pub(crate) entities: Vec<Entity>,
    /// The clause 6.3.17 warnings of the forwarding phases.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    pub(crate) contributions: Vec<SourceContribution>,
}

impl MergedEntities {
    /// The answer of a query no context source contributed to.
    pub(crate) const fn local(entities: Vec<Entity>) -> MergedEntities {
        MergedEntities {
            entities,
            warnings: Vec::new(),
            contributions: Vec::new(),
        }
    }
}

/// The two clause 5.7.2.4 forwarding phases of one query: the exclusive,
/// redirect and inclusive sources first, then the auxiliary ones (clause
/// 4.3.6.2).
pub(crate) struct QueryForwarding<'a> {
    /// The forwarding policy.
    pub(crate) forwarding: &'a FederationService,
    /// Live or Snapshot branch selected for the request.
    pub(crate) target: &'a ContextTarget,
    /// The JSON-LD engine the context source filter is expanded with.
    pub(crate) engine: &'a JsonLdEngine,
    /// The request URI problem responses name.
    pub(crate) uri: &'a HttpUri,
    /// The query as the client wrote it.
    pub(crate) query: &'a EntityQueryParameters,
    /// The resolved split-entities policy.
    pub(crate) split: SplitEntities,
    /// The request's JSON-LD context.
    pub(crate) context: &'a ContextSpec,
    /// The triggering request's `Via` chain and headers.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `EntityMap` the query runs under.
    pub(crate) entity_map: EntityMapInUse<'a>,
    /// The operation a registration has to declare for the query to reach it,
    /// which the binding the query arrived on decides (Table 5.2.9-1).
    pub(crate) operation: ForwardingOperation,
}

impl QueryForwarding<'_> {
    /// Run both phases.
    ///
    /// A source that could not be reached is a clause 6.3.17 warning, never a
    /// failure of the query; only the 501 of a lone proxied source is (Table
    /// 6.4.3.2-3).
    pub(crate) async fn run(&self) -> Result<ForwardedQueryResults, ProblemResponse> {
        // The CSR filter was parsed while extracting the request. The expanded
        // copy selects the registrations; the raw expression is forwarded as
        // the client wrote it (clause 5.7.2.4 propagates it into discovery).
        //
        // No `jsonKeys` declaration applies: Table 5.2.12-1 scopes that member
        // to Entity attribute names, while `csf` addresses the Context Source
        // Properties of a registration (clause 4.10), which no entity-level
        // declaration can name.
        let csf_expr: Option<Expr> = match &self.query.csf {
            Some(expr) => {
                let prepared_context = self.engine.prepare_term_context(self.context);
                let csf_expander = ContextualExpander::new(self.engine, &prepared_context);
                Some(
                    expand_q_expr(expr.clone(), &JsonKeyAttributes::default(), &csf_expander)
                        .await
                        .with_entity_instance(self.uri)?,
                ) // clone: the raw filter stays in the query for forwarding
            }
            None => None,
        };
        let primary = self.phase(CsrModeFilter::ExcludeAuxiliary, csf_expr.as_ref()).await;
        if let Some(status) = primary.sources.single_source_status {
            return Err(single_source_declined(status, self.uri));
        }
        let auxiliary = self.phase(CsrModeFilter::AuxiliaryOnly, csf_expr.as_ref()).await;

        let mut warnings = collect_warnings(&primary.results);
        warnings.extend(collect_warnings(&auxiliary.results));
        // Capture which registration returned which entity, and with which
        // map, before the merge (clauses 5.2.39 and 5.5.14).
        let mut contributions: Vec<SourceContribution> = contributions_of(&primary.results).collect();
        contributions.extend(contributions_of(&auxiliary.results));
        Ok(ForwardedQueryResults {
            primary: primary.results,
            auxiliary: auxiliary.results,
            warnings,
            contributions,
        })
    }

    /// One forwarding phase over the registrations `mode_filter` admits.
    async fn phase(&self, mode_filter: CsrModeFilter, csf_expr: Option<&Expr>) -> ForwardReadOutcome<ForwardedEntities> {
        self.forwarding
            .forward_query(
                self.target,
                ForwardQueryRequest {
                    client_query: ClientQuery::EntityQuery(self.query),
                    split: self.split,
                    context: self.context,
                    operation: self.operation,
                    triggering: self.triggering,
                    csf_expr,
                    mode_filter,
                    entity_map: self.entity_map,
                    source_page: None,
                },
            )
            .await
            .unwrap_or_else(|error| {
                warn!(%error, "context source forwarding failed during the entity query");
                ForwardReadOutcome::empty()
            })
    }
}
