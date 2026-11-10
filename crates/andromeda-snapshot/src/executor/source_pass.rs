//! One pass of a snapshot temporal capture over its Context Sources: every
//! offset page they answer over one window, read whole before any of it is
//! stored (clauses 4.12, 5.16.1.4 and 6.3.10).
//!
//! Query Temporal Evolution of Entities reads every source's answer to its
//! last page before it answers, charging every page to the budget of the
//! request (clause 4.12). A pass is that request: the first over the window the
//! query asked for, and each later one over the window resumed past a partial
//! answer, which a client following the range asks as a request of its own.
//! Every page of a pass is charged to one budget, and the pass is read whole
//! before the capture stores any of it, so a refused pass stores nothing.
//!
//! A forwarding that fails is read as Query Temporal Evolution of Entities
//! reads it: one this broker's own store failed fails the query, and any other
//! leaves the capture with the local answer.

use crate::executor::{
    SnapshotExecutor,
    execution_failure::ExecutionFailure,
    source_instance_walk::SourceInstanceWalk,
    status::failure_detail,
    temporal_capture::{TemporalCapture, capture_refused},
};
use andromeda_failure_classification::{
    forwarding_failure::local_federation_failure,
    result_limit_failure::{classify_instance_budget_refusal, classify_result_budget_refusal},
};
use andromeda_federation::{
    operation::ForwardingOperation,
    outcome::ForwardedTemporalEntities,
    query_forwarding_error::QueryForwardingError,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, SourcePage, SourcePaging, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    source_reach::SourceReach,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::collections::{HashSet, VecDeque};
use tracing::warn;

/// Every offset page the sources answered over one window, not yet stored.
pub(super) struct SourcePass {
    /// The pages, in the order they were asked for, each trimmed of what an
    /// earlier page or pass captured.
    pages: VecDeque<Vec<ForwardedTemporalEntities>>,
    /// The budget every page of the pass was charged to.
    budget: ResultBudget,
}

impl SourcePass {
    /// Whether a source answered an entity on any page of the pass.
    pub(super) fn holds_entities(&self) -> bool {
        self.answers().any(|answer| !answer.entities.is_empty())
    }

    /// The distinct entities the pass answers.
    pub(super) fn identifiers(&self) -> HashSet<&Identifier> {
        self.answers()
            .flat_map(|answer| answer.entities.iter().filter_map(|history| history.id()))
            .collect()
    }

    /// Whether a merged collection of `entities` distinct entities fits the
    /// entity limit the pass was charged under (clause 4.12).
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded::DistinctEntities`] when it does not.
    pub(super) fn admit_collection(&self, entities: usize) -> Result<(), ResultBudgetExceeded> {
        self.budget.admit_collection(u64::try_from(entities).unwrap_or(u64::MAX))
    }

    /// Take the next page of the pass, `None` once every page was taken.
    pub(super) fn next_page(&mut self) -> Option<Vec<ForwardedTemporalEntities>> {
        self.pages.pop_front()
    }

    /// Whether a page of the pass is still to be taken.
    pub(super) fn holds_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    /// Every source's answer on every page still held.
    fn answers(&self) -> impl Iterator<Item = &ForwardedTemporalEntities> {
        self.pages.iter().flatten()
    }
}

impl SnapshotExecutor {
    /// Ask the sources for every offset page of `params`, each trimmed by
    /// `walk` of what an earlier page or pass captured, under one budget.
    ///
    /// # Errors
    ///
    /// Returns the query's failure: the pass exceeds its budget, the
    /// forwarding failed in this broker's own store, a partial answer cannot be
    /// read on, or the pages run past the offsets a source can be asked for.
    pub(super) async fn gather_source_pass(
        &self,
        capture: &TemporalCapture<'_>,
        params: &TemporalQueryParameters,
        walk: &mut SourceInstanceWalk,
    ) -> Result<SourcePass, ExecutionResultDetails> {
        let budget = ResultBudget::new(self.sources.forwarding.result_limits());
        let mut pages = VecDeque::new();
        let mut offset = 0_u32;
        loop {
            let mut forwarded = self.forwarded_page(capture, params, offset, &budget).await?;
            let page_on = sources_page_on(&forwarded, self.capture_page_size);
            walk.trim(&mut forwarded)
                .map_err(|refusal| capture_refused(capture, &ExecutionFailure::SourceWalkRefused(refusal)))?;
            pages.push_back(forwarded);
            if !page_on {
                return Ok(SourcePass { pages, budget });
            }
            offset = offset
                .checked_add(self.capture_page_size)
                .ok_or_else(|| capture_refused(capture, &ExecutionFailure::OffsetOverflow))?;
        }
    }

    /// One offset page of what the registered context sources answer to
    /// `params`, charged to `budget`.
    ///
    /// # Errors
    ///
    /// Returns the query's failure when the page exceeds `budget` or the
    /// forwarding failed in this broker's own store.
    async fn forwarded_page(
        &self,
        capture: &TemporalCapture<'_>,
        params: &TemporalQueryParameters,
        offset: u32,
        budget: &ResultBudget,
    ) -> Result<Vec<ForwardedTemporalEntities>, ExecutionResultDetails> {
        let forwarded = self
            .sources
            .forwarding
            .forward_temporal_query(
                &self.sources.live_target,
                ForwardTemporalQueryRequest {
                    client_query: params,
                    split: self.sources.forwarding.split_entities(params.split_entities),
                    context: capture.context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    triggering: TriggeringRequest::default(),
                    csf_expr: capture.filters.csf(),
                    geo: capture.filters.geo(),
                    mode_filter: CsrModeFilter::All,
                    reach: SourceReach::Every,
                    entity_map: EntityMapInUse::None,
                    paging: SourcePaging::Page {
                        page: SourcePage {
                            limit: self.capture_page_size,
                            offset,
                        },
                        budget,
                    },
                },
            )
            .await;
        match forwarded {
            Ok(forwarded) => Ok(forwarded.results),
            Err(QueryForwardingError::TooManyResults(exceeded)) => Err(failure_detail(classify_result_budget_refusal(&exceeded).into_problem())),
            Err(QueryForwardingError::TooManyInstances(exceeded)) => Err(failure_detail(classify_instance_budget_refusal(&exceeded).into_problem())),
            Err(QueryForwardingError::Federation(error)) => {
                warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal CSR forwarding failed");
                match local_federation_failure(&error) {
                    Some(report) => Err(failure_detail(report.into_problem())),
                    None => Ok(Vec::new()),
                }
            }
        }
    }
}

/// Whether the sources may hold another offset page: one answered a full page,
/// or a partial one, which may leave out entities of a full page whose every
/// instance lies past its cut.
fn sources_page_on(forwarded: &[ForwardedTemporalEntities], page_size: u32) -> bool {
    forwarded
        .iter()
        .any(|source| source.entities.len() >= page_size as usize || !source.partial_ranges.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::executor::source_pass::{SourcePass, sources_page_on};
    use andromeda_federation::{
        outcome::ForwardedTemporalEntities,
        result_budget::{ResultBudget, ResultBudgetExceeded, ResultLimits},
    };
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_system_times::EntitySystemTimes,
        registration::forwarding_mode::ForwardingMode,
        temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
    };
    use indexmap::IndexMap;
    use std::{collections::VecDeque, num::NonZeroU32};

    fn vehicle(number: u32) -> TemporalHistory {
        let document = EntityDocument::new(
            Some(identifier(number)),
            vec!["https://example.com/Vehicle".parse().expect("a type")],
            None,
            None,
            IndexMap::new(),
        );
        TemporalHistory::Instances(TemporalDocument::new(document, EntitySystemTimes::default(), IndexMap::new()))
    }

    fn answer(numbers: &[u32]) -> ForwardedTemporalEntities {
        ForwardedTemporalEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid identifier")),
            mode: ForwardingMode::Inclusive,
            entities: numbers.iter().copied().map(vehicle).collect(),
            warnings: Vec::new(),
            linked_map: None,
            partial_ranges: Vec::new(),
        }
    }

    fn pass(pages: Vec<Vec<ForwardedTemporalEntities>>, entities: u32) -> SourcePass {
        SourcePass {
            pages: VecDeque::from(pages),
            budget: ResultBudget::new(ResultLimits {
                distinct_entities: NonZeroU32::new(entities).expect("a positive limit"),
                source_rows: NonZeroU32::MAX,
            }),
        }
    }

    fn identifier(number: u32) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{number}").parse().expect("a valid identifier")
    }

    #[test]
    fn an_entity_answered_on_several_pages_and_by_several_sources_is_one_entity_of_the_pass() {
        let pass = pass(vec![vec![answer(&[1, 2]), answer(&[2, 3])], vec![answer(&[3, 4])]], 10);

        let mut identifiers: Vec<&str> = pass.identifiers().into_iter().map(Identifier::as_str).collect();
        identifiers.sort_unstable();

        pretty_assertions::assert_eq!(
            identifiers,
            vec![
                "urn:ngsi-ld:Vehicle:1",
                "urn:ngsi-ld:Vehicle:2",
                "urn:ngsi-ld:Vehicle:3",
                "urn:ngsi-ld:Vehicle:4"
            ]
        );
        assert!(pass.holds_entities());
    }

    #[test]
    fn a_pass_whose_sources_answered_no_entity_holds_none() {
        let pass = pass(vec![vec![answer(&[])], vec![answer(&[])]], 10);

        assert!(!pass.holds_entities());
        assert!(pass.identifiers().is_empty());
    }

    /// Clause 4.12: the merged collection is refused only past the entity
    /// limit, so a collection of exactly the limit is admitted.
    #[test]
    fn a_merged_collection_is_admitted_up_to_the_entity_limit_of_the_pass() {
        let pass = pass(vec![vec![answer(&[1])]], 5);

        pretty_assertions::assert_eq!(pass.admit_collection(5), Ok(()));
        pretty_assertions::assert_eq!(
            pass.admit_collection(6),
            Err(ResultBudgetExceeded::DistinctEntities {
                maximum: NonZeroU32::new(5).expect("a positive limit")
            })
        );
    }

    #[test]
    fn the_pages_of_a_pass_are_taken_in_the_order_they_were_asked_for() {
        let mut pass = pass(vec![vec![answer(&[1])], vec![answer(&[2])]], 10);

        let first = pass.next_page().expect("a first page");
        assert!(pass.holds_pages());
        let second = pass.next_page().expect("a second page");

        pretty_assertions::assert_eq!(first[0].entities[0].id(), Some(&identifier(1)));
        pretty_assertions::assert_eq!(second[0].entities[0].id(), Some(&identifier(2)));
        assert!(!pass.holds_pages());
        assert!(pass.next_page().is_none());
    }

    #[test]
    fn the_sources_page_on_while_one_of_them_answers_a_full_page() {
        assert!(sources_page_on(&[answer(&[1]), answer(&[1, 2])], 2));
        assert!(!sources_page_on(&[answer(&[1]), answer(&[])], 2));
    }
}
