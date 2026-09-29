//! Capturing the result of one snapshot entity query (clause 5.16.1.4).
//!
//! Clause 5.16.1.4 executes a Snapshot's entity query "following the behaviour
//! described in clause 5.7.2.4. If the size of the respective results require
//! pagination, all pages are to be retrieved completely". The local store is
//! read page by page; every matching Context Source is walked once, to the last
//! page of its answer, before the first local page is written, because a
//! source's own pages carry no relation to the local ones: a source entity is
//! merged onto the local page that holds the same entity, and every source
//! entity no local page held is captured after the last one.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        execution::entity_query_failure,
        status::{failure_detail, success_detail},
        translation::entity_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    entity_budget::{EntityBudget, EntityBudgetExceeded},
    entity_result::collect_warnings,
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    outcome::ForwardedEntities,
    query_forwarding_error::QueryForwardingError,
    request::{CsrModeFilter, EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, query::body::Query, snapshot::ExecutionResultDetails};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use std::collections::HashSet;
use tracing::warn;

/// What walking every matching Context Source came to.
enum WalkedSources {
    /// Every source answered, or failed with a warning of its own.
    Answered(PendingSources),
    /// The forwarding could not run at all.
    Unreachable(FederationError),
    /// The sources answered with more entities than one query gathers.
    TooManyResults(EntityBudgetExceeded),
}

/// What every matching Context Source answered, not yet captured.
struct PendingSources {
    /// The answers, each still holding the entities no local page has claimed.
    answers: Vec<ForwardedEntities>,
}

impl PendingSources {
    /// Take out of every answer the entities one local page holds, keeping
    /// each answer's registration and mode so the merge treats them as the
    /// answer they came from (clause 4.3.6.2).
    fn claim(&mut self, local: &[Entity]) -> Vec<ForwardedEntities> {
        let held: HashSet<&str> = local.iter().map(|entity| entity.id.as_str()).collect();
        self.answers
            .iter_mut()
            .filter_map(|answer| {
                let (claimed, kept): (Vec<Entity>, Vec<Entity>) = answer.entities.drain(..).partition(|entity| held.contains(entity.id.as_str()));
                answer.entities = kept;
                (!claimed.is_empty()).then(|| ForwardedEntities {
                    registration_id: answer.registration_id.clone(), // clone: the claimed part names the registration the rest still names
                    mode: answer.mode,
                    entities: claimed,
                    warnings: Vec::new(),
                    linked_map: None,
                })
            })
            .collect()
    }

    /// Whether any answer still holds an entity.
    fn holds_entities(&self) -> bool {
        self.answers.iter().any(|answer| !answer.entities.is_empty())
    }
}

/// The identifiers a capture has written, for the query and across the
/// Snapshot.
struct CaptureLedger<'a> {
    /// Every entity this query captured.
    captured: HashSet<Identifier>,
    /// Every entity any query of the Snapshot has materialized so far.
    materialized: &'a mut HashSet<Identifier>,
}

impl SnapshotExecutor {
    /// Execute one snapshot entity query and capture its whole result.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure a local query page or a staging write
    /// met; the query's own failures are recorded in the returned details.
    pub(crate) async fn run_entity_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
        materialized_entities: &mut HashSet<Identifier>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let mut params = match entity_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        if let Err(error) = params.validate() {
            return Ok(failure_detail(format!("Snapshot query translation failed validation: {error}")));
        }

        let (mut pending, forwarding_failure) = match self.walk_sources(&params, context).await {
            WalkedSources::Answered(pending) => (pending, None),
            WalkedSources::Unreachable(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot CSR forwarding failed");
                (PendingSources { answers: Vec::new() }, Some(error))
            }
            WalkedSources::TooManyResults(exceeded) => return Ok(failure_detail(exceeded.to_string())),
        };
        for warning in pending.answers.iter().flat_map(|answer| answer.warnings.iter()) {
            warn!(snapshot_id = %snapshot_id, query_index = index, warning = %warning, "Snapshot entity query retained a distributed warning");
        }
        let every_source_failed = !collect_warnings(&pending.answers).is_empty() && !pending.holds_entities();

        let mut ledger = CaptureLedger {
            captured: HashSet::new(),
            materialized: materialized_entities,
        };
        let mut offset = 0_u32;
        loop {
            params.limit = Some(self.capture_page_size);
            params.offset = Some(offset);
            params.count = Some(true);
            let (entities, _) = match self.sources.entity_query.query(&self.sources.live_target, &params, context).await {
                Ok(result) => result,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                    return entity_query_failure(error);
                }
            };
            let local_page_len = entities.len();
            if offset == 0 && local_page_len == 0 {
                if let Some(error) = &forwarding_failure {
                    return Ok(failure_detail(format!(
                        "No valid entity result was available because distributed execution failed: {error}"
                    )));
                }
                if every_source_failed {
                    return Ok(failure_detail(
                        "No valid entity result was available because every matching distributed source failed",
                    ));
                }
            }
            let claimed = pending.claim(&entities);
            let entities = if claimed.is_empty() {
                entities
            } else {
                EntityQueryService::merge_with_forwarded(entities, claimed, None)
            };
            if let Some(failure) = self.capture_page(target, snapshot_id, index, context, entities, &mut ledger).await? {
                return Ok(failure);
            }
            if local_page_len < self.capture_page_size as usize {
                break;
            }
            offset = match offset.checked_add(self.capture_page_size) {
                Some(offset) => offset,
                None => return Ok(failure_detail("Snapshot entity pagination offset overflowed")),
            };
        }

        // The source entities no local page held, merged across sources.
        let remaining = EntityQueryService::merge_with_forwarded(Vec::new(), pending.answers, None);
        let mut remaining = remaining.into_iter().peekable();
        while remaining.peek().is_some() {
            let page = remaining.by_ref().take(self.capture_page_size as usize).collect();
            if let Some(failure) = self.capture_page(target, snapshot_id, index, context, page, &mut ledger).await? {
                return Ok(failure);
            }
        }
        Ok(success_detail(ledger.captured.len()))
    }

    /// Walk every matching Context Source to the end of its answer (clause
    /// 5.16.1.4).
    async fn walk_sources(&self, params: &EntityQueryParameters, context: &ContextSpec) -> WalkedSources {
        let budget = EntityBudget::new(self.sources.forwarding.max_query_entities());
        let forwarded = self
            .sources
            .forwarding
            .forward_query(
                &self.sources.live_target,
                ForwardQueryRequest {
                    client_query: ClientQuery::EntityQuery(params),
                    // A Snapshot materialises its members through the GET query
                    // binding (clause 5.5.15).
                    delivery: QueryDelivery::Query,
                    split: self.sources.forwarding.split_entities(params.split_entities),
                    context,
                    triggering: TriggeringRequest::default(),
                    csf_expr: None,
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                    budget: &budget,
                },
            )
            .await;
        match forwarded {
            Ok(outcome) => WalkedSources::Answered(PendingSources { answers: outcome.results }),
            Err(QueryForwardingError::TooManyResults(exceeded)) => WalkedSources::TooManyResults(exceeded),
            Err(QueryForwardingError::Federation(error)) => WalkedSources::Unreachable(error),
        }
    }

    /// Expand and persist one page of captured entities, skipping those an
    /// earlier query of the Snapshot already materialized, and answering the
    /// failure recorded for the query when the page could not be written.
    async fn capture_page(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        context: &ContextSpec,
        entities: Vec<Entity>,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let mut pairs = Vec::with_capacity(entities.len());
        for entity in entities {
            let identifier = entity.id.clone(); // clone: the atomic write and result accounting independently own the identifier.
            let compact = match serde_json::to_value(entity) {
                Ok(compact) => compact,
                Err(error) => return Ok(Some(failure_detail(format!("Failed to serialize captured entity: {error}")))),
            };
            let expanded = match self.sources.entity_query.engine().expand(compact, context).await {
                Ok(expanded) => expanded,
                Err(error) => return Ok(Some(failure_detail(format!("Failed to expand captured entity: {error}")))),
            };
            pairs.push((identifier, expanded));
        }
        let new_pairs = pairs.iter().filter(|(id, _)| !ledger.materialized.contains(id)).cloned().collect::<Vec<_>>();
        let detail = self.persist_current_results(target, snapshot_id, index, &new_pairs).await?;
        if detail.problem_details.is_some() {
            return Ok(Some(detail));
        }
        ledger.captured.extend(pairs.into_iter().map(|(id, _)| id));
        ledger.materialized.extend(new_pairs.into_iter().map(|(id, _)| id));
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::entity_capture::PendingSources;
    use andromeda_federation::outcome::ForwardedEntities;
    use andromeda_ngsi_ld_model::{entity::Entity, registration::forwarding_mode::ForwardingMode};
    use serde_json::json;

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Vehicle"})).expect("a valid entity")
    }

    fn answer(ids: &[&str]) -> ForwardedEntities {
        ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: ids.iter().map(|id| entity(id)).collect(),
            warnings: Vec::new(),
            linked_map: None,
        }
    }

    fn ids(answers: &[ForwardedEntities]) -> Vec<&str> {
        answers
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|entity| entity.id.as_str()))
            .collect()
    }

    /// A local page claims exactly the source entities it holds; the others
    /// stay pending for a later page or for the capture after the last one.
    #[test]
    fn a_local_page_claims_only_the_source_entities_it_holds() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:a", "urn:b", "urn:c"]), answer(&["urn:c", "urn:d"])],
        };
        let claimed = pending.claim(&[entity("urn:c"), entity("urn:x")]);
        pretty_assertions::assert_eq!(ids(&claimed), vec!["urn:c", "urn:c"]);
        pretty_assertions::assert_eq!(ids(&pending.answers), vec!["urn:a", "urn:b", "urn:d"]);
        assert!(pending.holds_entities());
    }

    /// Once every source entity is claimed nothing is pending.
    #[test]
    fn nothing_is_pending_once_every_source_entity_is_claimed() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:a"])],
        };
        let claimed = pending.claim(&[entity("urn:a")]);
        pretty_assertions::assert_eq!(ids(&claimed), vec!["urn:a"]);
        assert!(!pending.holds_entities());
    }
}
