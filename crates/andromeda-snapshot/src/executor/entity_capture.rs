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
//!
//! A local entity is captured as the stored state the query selected, written
//! into the Snapshot's branch as stored. Only an entity a source also answered
//! for, or one only a source holds, goes through the compact merge
//! ([`crate::executor::forwarded_capture`]).

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        captured_state::narrow_captured,
        execution::entity_query_failure,
        forwarded_capture::{ForwardedCapture, MergedCaptureError},
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
    instance_budget::InstanceBudgetExceeded,
    outcome::ForwardedEntities,
    query_forwarding_error::QueryForwardingError,
    request::{CsrModeFilter, EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, query::body::Query, snapshot::ExecutionResultDetails};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, sync::Arc};
use tracing::warn;

/// What walking every matching Context Source came to.
enum WalkedSources {
    /// Every source answered, or failed with a warning of its own.
    Answered(PendingSources),
    /// The forwarding could not run at all.
    Unreachable(FederationError),
    /// The sources answered with more entities than one query gathers.
    TooManyResults(EntityBudgetExceeded),
    /// The sources answered with more attribute instances than one query
    /// gathers.
    TooManyInstances(InstanceBudgetExceeded),
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
    fn claim<'i>(&mut self, local: impl IntoIterator<Item = &'i Identifier>) -> Vec<ForwardedEntities> {
        let held: HashSet<&Identifier> = local.into_iter().collect();
        self.answers
            .iter_mut()
            .filter_map(|answer| {
                let (claimed, kept): (Vec<Entity>, Vec<Entity>) = answer.entities.drain(..).partition(|entity| held.contains(&entity.id));
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
        let params = match entity_parameters(query) {
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
            WalkedSources::TooManyInstances(exceeded) => return Ok(failure_detail(exceeded.to_string())),
        };
        for warning in pending.answers.iter().flat_map(|answer| answer.warnings.iter()) {
            warn!(snapshot_id = %snapshot_id, query_index = index, warning = %warning, "Snapshot entity query retained a distributed warning");
        }
        let every_source_failed = !collect_warnings(&pending.answers).is_empty() && !pending.holds_entities();

        let capture = ForwardedCapture::new(self.sources.entity_query.engine(), context);
        let mut ledger = CaptureLedger {
            captured: HashSet::new(),
            materialized: materialized_entities,
        };
        // The local entities are walked by keyset over their identifier rather
        // than by offset: entities written or deleted while the capture runs
        // then neither shift a later page nor appear twice (clause 5.5.9.1).
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        loop {
            let (rows, _) = match self
                .sources
                .entity_query
                .query_states(&self.sources.live_target, &params, &window, CountPolicy::Omit, context)
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                    return entity_query_failure(error);
                }
            };
            // The page statement decoded each state into its own allocation,
            // so taking it out of its `Arc` never copies; the entity-level
            // system times read beside it are not part of a captured entity.
            let mut states: Vec<EntityState> = rows.into_iter().map(|row| Arc::unwrap_or_clone(row.state)).collect();
            let local_page_len = states.len();
            // clone: the next page starts after the last identifier of this one,
            // while the states themselves move into the capture below.
            let last_captured = states.last().map(|state| state.id().clone());
            if matches!(window.start, PageStart::Cursor(None)) && local_page_len == 0 {
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
            let captured_at = Utc::now();
            for state in &mut states {
                narrow_captured(state, captured_at, params.dataset_id.as_deref());
            }
            let claimed = pending.claim(states.iter().map(EntityState::id));
            let states = match merge_claimed(&capture, states, claimed, captured_at).await {
                Ok(states) => states,
                Err(error) => return Ok(failure_detail(error.to_string())),
            };
            if let Some(failure) = self.capture_page(target, snapshot_id, index, states, &mut ledger).await? {
                return Ok(failure);
            }
            match last_captured {
                Some(last) if local_page_len >= self.capture_page_size as usize => window = PageWindow::after(self.capture_page_size, last),
                Some(_) | None => break,
            }
        }

        // The source entities no local page held, merged across sources.
        let remaining = EntityQueryService::merge_with_forwarded(Vec::new(), pending.answers, None);
        let mut remaining = remaining.into_iter().peekable();
        while remaining.peek().is_some() {
            let page = remaining.by_ref().take(self.capture_page_size as usize).collect();
            let states = match capture.states(page).await {
                Ok(states) => states,
                Err(error) => return Ok(failure_detail(error.to_string())),
            };
            if let Some(failure) = self.capture_page(target, snapshot_id, index, states, &mut ledger).await? {
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
            Err(QueryForwardingError::TooManyInstances(exceeded)) => WalkedSources::TooManyInstances(exceeded),
            Err(QueryForwardingError::Federation(error)) => WalkedSources::Unreachable(error),
        }
    }

    /// Persist one page of captured states, skipping those an earlier query of
    /// the Snapshot already materialized, and answering the failure recorded
    /// for the query when the page could not be written.
    async fn capture_page(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        states: Vec<EntityState>,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        // clone: the ledger keeps every captured identifier once the states move into the branch write
        let identifiers = states.iter().map(|state| state.id().clone()).collect::<Vec<_>>();
        let fresh = states.into_iter().filter(|state| !ledger.materialized.contains(state.id())).collect::<Vec<_>>();
        let detail = self.persist_current_results(target, snapshot_id, index, fresh).await?;
        if detail.problem_details.is_some() {
            return Ok(Some(detail));
        }
        for identifier in identifiers {
            ledger.materialized.insert(identifier.clone()); // clone: both ledgers own the identifier
            ledger.captured.insert(identifier);
        }
        Ok(None)
    }
}

/// One local page's states, the ones a source also answered for merged with
/// what it answered; the others stay as stored.
async fn merge_claimed(
    capture: &ForwardedCapture<'_>,
    states: Vec<EntityState>,
    claimed: Vec<ForwardedEntities>,
    captured_at: DateTime<Utc>,
) -> Result<Vec<EntityState>, MergedCaptureError> {
    if claimed.is_empty() {
        return Ok(states);
    }
    let (answered, unanswered): (Vec<EntityState>, Vec<EntityState>) = {
        let answered_ids: HashSet<&Identifier> = claimed.iter().flat_map(|answer| answer.entities.iter().map(|entity| &entity.id)).collect();
        states.into_iter().partition(|state| answered_ids.contains(state.id()))
    };
    let mut merged = capture.merged(&answered, claimed, captured_at).await?;
    merged.extend(unanswered);
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use crate::executor::entity_capture::PendingSources;
    use andromeda_federation::outcome::ForwardedEntities;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{entity::Entity, registration::forwarding_mode::ForwardingMode};
    use serde_json::json;

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Vehicle"})).expect("a valid entity")
    }

    fn identifier(id: &str) -> Identifier {
        id.parse().expect("a valid identifier")
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
        let local = [identifier("urn:c"), identifier("urn:x")];
        let claimed = pending.claim(&local);
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
        let local = [identifier("urn:a")];
        let claimed = pending.claim(&local);
        pretty_assertions::assert_eq!(ids(&claimed), vec!["urn:a"]);
        assert!(!pending.holds_entities());
    }
}
