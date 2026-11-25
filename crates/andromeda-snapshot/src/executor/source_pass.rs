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
//! The pass holds the pages as the sources answered them until it is settled:
//! a split query decides its filters over those answers (clause 5.7.4.4).
//! Settling leaves out the entities the decision dropped, then what an earlier
//! page or pass captured.
//!
//! A forwarding that fails is read as Query Temporal Evolution of Entities
//! reads it: one this broker's own store failed fails the query, and any other
//! leaves the capture with the local answer. A source that fails leaves the
//! pass with what the others answered, and the warning that names it.

use crate::executor::{
    SnapshotExecutor,
    execution_failure::ExecutionFailure,
    source_instance_walk::{SourceInstanceWalk, refuse_unpositioned},
    split_verdict::SplitVerdict,
    status::failure_detail,
    temporal_capture::{TemporalCapture, capture_refused},
};
use andromeda_failure_classification::{
    forwarding_failure::local_federation_failure,
    result_limit_failure::{classify_instance_budget_refusal, classify_result_budget_refusal},
};
use andromeda_federation::{
    in_effect_probe::InEffectReading,
    operation::ForwardingOperation,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    query_forwarding_error::QueryForwardingError,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, SourcePage, SourcePaging, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    source_reach::SourceReach,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    snapshot::ExecutionResultDetails,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::collections::{HashSet, VecDeque};
use tracing::warn;

/// Every offset page the sources answered over one window, not yet stored.
pub(super) struct SourcePass {
    /// The pages, in the order they were asked for, none of them cut short at
    /// no readable range: as the sources answered them until the pass is
    /// settled, then each trimmed of what the pass dropped and of what an
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

    /// The instances of every entity part the pass answers.
    pub(super) fn answered_documents(&self) -> impl Iterator<Item = &TemporalDocument> {
        self.answers().flat_map(|answer| answer.entities.iter()).filter_map(TemporalHistory::instances)
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

    /// Leave out of every page the entities `verdict` drops, then, page after
    /// page, what `walk` says an earlier page or pass captured.
    ///
    /// A dropped entity is left out before the walk reads the page, so the
    /// walk remembers only what the capture stores.
    pub(super) fn settle(&mut self, verdict: &SplitVerdict, walk: &mut SourceInstanceWalk) {
        for page in &mut self.pages {
            for answer in page.iter_mut() {
                answer.entities.retain(|history| history.id().is_none_or(|entity_id| verdict.keeps(entity_id)));
            }
            walk.trim(page);
        }
    }

    /// Take the next page of the pass, `None` once every page was taken.
    pub(super) fn next_page(&mut self) -> Option<Vec<ForwardedTemporalEntities>> {
        self.pages.pop_front()
    }

    /// Whether a page of the pass is still to be taken.
    pub(super) fn holds_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    /// The clause 6.3.17 warnings every source answered with on the pages
    /// still held.
    pub(super) fn warnings(&self) -> impl Iterator<Item = &NgsiLdWarning> {
        self.answers().flat_map(|answer| answer.warnings.iter())
    }

    /// Every source's answer on every page still held.
    fn answers(&self) -> impl Iterator<Item = &ForwardedTemporalEntities> {
        self.pages.iter().flatten()
    }
}

impl SnapshotExecutor {
    /// Ask the sources for every offset page of `params` under one budget.
    ///
    /// # Errors
    ///
    /// Returns the query's failure: the pass exceeds its budget, the
    /// forwarding failed in this broker's own store, a partial answer names no
    /// instant to read on from, or the pages run past the offsets a source can
    /// be asked for.
    pub(super) async fn gather_source_pass(
        &self,
        capture: &TemporalCapture<'_>,
        params: &TemporalQueryParameters,
    ) -> Result<SourcePass, ExecutionResultDetails> {
        let budget = ResultBudget::new(self.sources.forwarding.result_limits());
        let mut pages = VecDeque::new();
        let mut offset = 0_u32;
        loop {
            let page = SourcePaging::Page {
                page: SourcePage {
                    limit: self.capture_page_size,
                    offset,
                },
                budget: &budget,
            };
            let forwarded = self
                .sources
                .forwarding
                .forward_temporal_query(&self.sources.live_target, self.forwarded_request(capture, params, page))
                .await;
            let forwarded = forwarded_results(capture, forwarded)?;
            refuse_unpositioned(&forwarded).map_err(|refusal| capture_refused(capture, &ExecutionFailure::SourceWalkRefused(refusal)))?;
            let page_on = sources_page_on(&forwarded, self.capture_page_size);
            pages.push_back(forwarded);
            if !page_on {
                return Ok(SourcePass { pages, budget });
            }
            offset = offset
                .checked_add(self.capture_page_size)
                .ok_or_else(|| capture_refused(capture, &ExecutionFailure::OffsetOverflow))?;
        }
    }

    /// What the sources answer the probes for the Scope in force when the
    /// window of `params` opens (clauses 4.18 and 5.7.4.4), each walked to its
    /// last page under a budget of its own, as Query Temporal Evolution of
    /// Entities probes them; none when the query carries no Scope query or its
    /// window opens where no Scope set before it can be in force.
    ///
    /// # Errors
    ///
    /// Returns the query's failure when a probe exceeds its budget or the
    /// forwarding failed in this broker's own store.
    pub(super) async fn in_effect_answers(
        &self,
        capture: &TemporalCapture<'_>,
        params: &TemporalQueryParameters,
    ) -> Result<Vec<ForwardedTemporalEntities>, ExecutionResultDetails> {
        let mut answers = Vec::new();
        for reading in [InEffectReading::Placed, InEffectReading::Deletion] {
            let budget = ResultBudget::new(self.sources.forwarding.result_limits());
            let forwarded = self
                .sources
                .forwarding
                .forward_in_effect_probe(
                    &self.sources.live_target,
                    self.forwarded_request(capture, params, SourcePaging::Walk(&budget)),
                    reading,
                )
                .await;
            answers.extend(forwarded_results(capture, forwarded)?);
        }
        Ok(answers)
    }

    /// The forwarding of `params` to every registration the query reaches,
    /// read as `paging` says.
    fn forwarded_request<'r>(
        &'r self,
        capture: &'r TemporalCapture<'_>,
        params: &'r TemporalQueryParameters,
        paging: SourcePaging<'r>,
    ) -> ForwardTemporalQueryRequest<'r> {
        ForwardTemporalQueryRequest {
            client_query: params,
            split: self.sources.forwarding.split_entities(params.split_entities),
            context: capture.context,
            operation: ForwardingOperation::QueryTemporalEntity,
            triggering: TriggeringRequest::default(),
            csf: capture.filters.csf(),
            geo: capture.filters.geo(),
            mode_filter: CsrModeFilter::All,
            reach: SourceReach::Every,
            entity_map: EntityMapInUse::None,
            paging,
        }
    }
}

/// What the sources answered a forwarding with.
///
/// # Errors
///
/// Returns the query's failure when the answers exceed their budget or the
/// forwarding failed in this broker's own store; any other failure leaves the
/// capture with no answer.
fn forwarded_results(
    capture: &TemporalCapture<'_>,
    forwarded: Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError>,
) -> Result<Vec<ForwardedTemporalEntities>, ExecutionResultDetails> {
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

/// Whether the sources may hold another offset page: one answered a full page,
/// or a partial one, which may leave out entities of a full page whose every
/// instance lies past its cut.
fn sources_page_on(forwarded: &[ForwardedTemporalEntities], page_size: u32) -> bool {
    forwarded
        .iter()
        .any(|source| source.entities.len() >= page_size as usize || !source.partial_pages.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::executor::{
        source_instance_walk::SourceInstanceWalk,
        source_pass::{SourcePass, sources_page_on},
        split_verdict::SplitVerdict,
    };
    use andromeda_federation::{
        outcome::ForwardedTemporalEntities,
        result_budget::{ResultBudget, ResultBudgetExceeded, ResultLimits},
    };
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{
        entity_system_times::EntitySystemTimes,
        registration::forwarding_mode::ForwardingMode,
        temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_query::temporal::{
        contributed_entities::{ContributedEntities, ContributedRendering},
        window::TimeInterval,
    };
    use andromeda_temporal::split_settlement::SplitSettlement;
    use chrono::DateTime;
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
            partial_pages: Vec::new(),
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
    fn a_pass_holds_the_warnings_of_every_answer_on_every_page() {
        let failed = |registration: u32| NgsiLdWarning::new(WarnCode::MiscellaneousWarning, format!("registration {registration} did not respond"));
        let warned = |registration: u32| ForwardedTemporalEntities {
            warnings: vec![failed(registration)],
            ..answer(&[])
        };
        let pass = pass(vec![vec![answer(&[1]), warned(1)], vec![warned(2)]], 10);

        pretty_assertions::assert_eq!(pass.warnings().cloned().collect::<Vec<_>>(), vec![failed(1), failed(2)]);
    }

    #[test]
    fn the_sources_page_on_while_one_of_them_answers_a_full_page() {
        assert!(sources_page_on(&[answer(&[1]), answer(&[1, 2])], 2));
        assert!(!sources_page_on(&[answer(&[1]), answer(&[])], 2));
    }

    /// Clause 5.7.4.4: a split pass judges the parts its sources answered, so
    /// they are held as answered until it is settled, and settling leaves out
    /// every part of an entity the filters dropped.
    #[test]
    fn a_settled_pass_leaves_out_the_entities_its_verdict_dropped() {
        let mut pass = pass(vec![vec![answer(&[1, 2])], vec![answer(&[1, 3])]], 10);
        pretty_assertions::assert_eq!(pass.answered_documents().count(), 4);
        let verdict = SplitVerdict::new(
            ContributedEntities::new([identifier(1), identifier(2), identifier(3)], ContributedRendering::Local),
            SplitSettlement {
                retained: [identifier(2), identifier(3)].into_iter().collect(),
                incomplete: Vec::new(),
                narrowing: None,
            },
        );
        let mut walk = SourceInstanceWalk::new(TimeInterval::After(DateTime::UNIX_EPOCH), TimeProp::Observed, None);

        pass.settle(&verdict, &mut walk);

        let kept = |page: Vec<ForwardedTemporalEntities>| -> Vec<Identifier> {
            page.iter()
                .flat_map(|answer| &answer.entities)
                .filter_map(TemporalHistory::id)
                .cloned()
                .collect()
        };
        pretty_assertions::assert_eq!(kept(pass.next_page().expect("a first page")), vec![identifier(2)]);
        pretty_assertions::assert_eq!(kept(pass.next_page().expect("a second page")), vec![identifier(3)]);
    }
}
