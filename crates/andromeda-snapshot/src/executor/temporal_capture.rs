//! Capturing the whole answer of one snapshot temporal query (clause
//! 5.16.1.4).
//!
//! The context sources are asked first: every offset page of the window the
//! query asked for is read whole, charged to one budget as Query Temporal
//! Evolution of Entities charges the sources of one request (clause 4.12). The
//! local entities are then paged, and each local page is stored beside the
//! source page read in the same position. The instances of each page of local
//! entities are read to the end of the window, page after page (clause
//! 6.3.10). What the sources cut short is read in further passes over every
//! source page, each over the window resumed past the previous pass.
//!
//! Once a source answered an entity, Query Temporal Evolution of Entities
//! refuses a merged collection past the entity limit (clause 4.12), counting
//! an entity once however many sources and pages answer it. The capture then
//! reads the identifier of every local entity before it stores anything, and
//! refuses the query when they and the sources' entities together pass the
//! limit; a later pass holding a source entity is held to it with every entity
//! already captured. A capture no source answered an entity for pages its
//! local entities unbounded, as Query Temporal Evolution of Entities pages
//! them.
//!
//! Under `splitEntities` each pass decides the query's values filter,
//! geoquery and Scope query over the gathered parts of every entity its
//! sources answered, and stores none of an entity those filters drop (clause
//! 5.7.4.4). A later pass may keep an entity the capture holds nothing of yet:
//! its part held here is then read over that pass's window, as Query Temporal
//! Evolution of Entities reads it for a client following the range.
//!
//! A Context Source that fails does not fail the query, as it does not fail
//! Query Temporal Evolution of Entities: the capture holds what the broker and
//! the other sources answered, and its execution result names every source
//! that failed in any pass.
//!
//! A failure met after a page was stored leaves that page in the staging
//! branch: the query's capture says it wrote, and the Snapshot is captured
//! again without the query.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        execution_failure::ExecutionFailure,
        query_capture::{QueryCapture, Staging},
        source_instance_walk::SourceInstanceWalk,
        source_shortfall::SourceShortfall,
        split_verdict::{FilterGathering, PassJudgement, SplitVerdict},
        status::{answered_detail, failure_detail},
        temporal_failure::{TemporalStep, temporal_failure},
        temporal_local_entities::{LocalAdmission, LocalEntities},
        translation::temporal_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_failure_classification::{
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
    result_limit_failure::classify_result_budget_refusal,
};
use andromeda_federation::{expanded_registration_filters::ExpandedRegistrationFilters, outcome::ForwardedTemporalEntities};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{query::body::Query, snapshot::ExecutionResultDetails, temporal::temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::{query_error::QueryError, temporal::window::TimeInterval};
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, admitted::AdmittedTemporalRequest, operation::TemporalQueryOperation};
use std::collections::HashSet;
use tracing::warn;

/// One snapshot temporal query, as every page of its capture reads it.
pub(super) struct TemporalCapture<'q> {
    /// The Snapshot's staging branch.
    pub(super) target: &'q ContextTarget,
    /// The Snapshot being prepared.
    pub(super) snapshot_id: &'q Identifier,
    /// The query's position in `snapshotTemporalQueries`.
    pub(super) index: usize,
    /// The query as the Snapshot asked it.
    pub(super) params: &'q AdmittedTemporalRequest,
    /// The window the query asked for.
    pub(super) interval: TimeInterval,
    /// The context the query is expanded under.
    pub(super) context: &'q ContextSpec,
    /// The query's context source filter and geoquery, which select the
    /// registrations it reaches.
    pub(super) filters: &'q ExpandedRegistrationFilters,
    /// Where the query's values filter, geoquery and Scope query are decided.
    pub(super) gathering: FilterGathering,
}

/// One pass of a capture: the request it stands for, over the pass's own
/// window, and what it decided of its split filters.
#[derive(Clone, Copy)]
pub(super) struct CapturePass<'p> {
    /// The query over the pass's window.
    pub(super) params: &'p AdmittedTemporalRequest,
    /// The pass's window.
    pub(super) interval: TimeInterval,
    /// What the pass decided of its split filters.
    pub(super) verdict: &'p SplitVerdict,
}

impl SnapshotExecutor {
    /// Execute one snapshot temporal query and capture its whole result,
    /// paging the local history and the forwarded sources side by side.
    ///
    /// # Errors
    ///
    /// Returns a transient failure a local page read or a history write met,
    /// on which the preparation restarts; the query's own failures are
    /// recorded in the returned capture.
    pub(super) async fn run_temporal_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
    ) -> Result<QueryCapture, SnapshotError> {
        let mut captured = HashSet::new();
        let detail = self.capture_temporal_result(target, snapshot_id, query, index, context, &mut captured).await?;
        // Every entity of `captured` was handed to the staging branch with the
        // page it came in.
        let staging = if captured.is_empty() { Staging::Untouched } else { Staging::Written };
        Ok(QueryCapture::of(detail, staging))
    }

    /// Capture the whole result of one snapshot temporal query, every entity
    /// a page of it stores added to `captured`.
    ///
    /// # Errors
    ///
    /// As [`SnapshotExecutor::run_temporal_query`].
    async fn capture_temporal_result(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
        captured: &mut HashSet<Identifier>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let params = match temporal_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(classify_parameter_error(&error).into_problem())),
        };
        let interval = match TimeInterval::from_parts(params.timerel, params.time_at, params.end_time_at) {
            Ok(interval) => interval,
            Err(error) => return Ok(failure_detail(classify_query_error(&QueryError::from(error)).into_problem())),
        };
        let temporal = &self.sources.temporal;
        let params = match params.admit(TemporalQueryOperation::Query, temporal.linked_retrieval(), temporal.filter_admission()) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(classify_parameter_error(&error).into_problem())),
        };
        let filters = match self.temporal_registration_filters(&params, context).await {
            Ok(filters) => filters,
            Err(error) => return Ok(failure_detail(classify_query_error(&error).into_problem())),
        };
        let capture = TemporalCapture {
            target,
            snapshot_id,
            index,
            params: &params,
            interval,
            context,
            filters: &filters,
            gathering: FilterGathering::of(&params, self.sources.forwarding.split_entities(params.split_entities)),
        };
        let mut sources = SourceInstanceWalk::new(interval, params.timeproperty_or_default(), params.last_n);
        let mut shortfall = SourceShortfall::default();
        if let Some(failure) = self.capture_first_pass(&capture, &mut sources, &mut shortfall, captured).await? {
            return Ok(failure);
        }
        if let Some(failure) = self.capture_resumed_source_passes(&capture, sources, &mut shortfall, captured).await? {
            return Ok(failure);
        }
        Ok(answered_detail(captured.len(), &shortfall))
    }

    /// The query's context source filter and geoquery expanded under `context`,
    /// which select the registrations the query reaches exactly as Query
    /// Temporal Evolution of Entities selects them (clauses 4.3.6.1, 4.9 and
    /// 5.10.2.4). A query limited to the local scope reaches none.
    async fn temporal_registration_filters(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<ExpandedRegistrationFilters, QueryError> {
        if params.is_local() {
            return Ok(ExpandedRegistrationFilters::default());
        }
        let temporal = &self.sources.temporal;
        ExpandedRegistrationFilters::expand(temporal.engine(), context, params.csf.as_ref(), params.geo.0.as_ref(), temporal.geo_admission()).await
    }

    /// Capture the first pass over the window the query asked for: every
    /// local page beside the source page read in the same position. The
    /// failed sources it met are recorded in `shortfall`.
    ///
    /// Returns the query's failure, or `None` once every page is stored.
    async fn capture_first_pass(
        &self,
        capture: &TemporalCapture<'_>,
        sources: &mut SourceInstanceWalk,
        shortfall: &mut SourceShortfall,
        captured: &mut HashSet<Identifier>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let mut pass = match self.gather_source_pass(capture, capture.params).await {
            Ok(pass) => pass,
            Err(failure) => return Ok(Some(failure)),
        };
        shortfall.record(pass.warnings());
        let verdict = match self.judge_pass(capture, capture.params, &pass, shortfall).await? {
            PassJudgement::Judged(verdict) => verdict,
            PassJudgement::Ended(failure) => return Ok(Some(failure)),
        };
        let mut local = if pass.holds_entities() {
            match self.admitted_local_entities(capture, &pass, verdict.contributed()).await? {
                LocalAdmission::Admitted(identifiers) => LocalEntities::Admitted(identifiers.into_iter()),
                LocalAdmission::Ended(failure) => return Ok(Some(failure)),
            }
        } else {
            LocalEntities::from_first(self.capture_page_size)
        };
        pass.settle(&verdict, sources);
        let request = CapturePass {
            params: capture.params,
            interval: capture.interval,
            verdict: &verdict,
        };
        loop {
            let page = match self.next_local_page(capture, &mut local, verdict.contributed()).await {
                Ok(page) => page,
                Err(error) => {
                    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal query failed");
                    return temporal_failure(error, TemporalStep::Query).map(Some);
                }
            };
            let local_on = local.continues_after(&page, self.capture_page_size);
            let entity_ids = page.into_iter().filter(|entity_id| verdict.keeps(entity_id)).collect();
            let forwarded = pass.next_page().unwrap_or_default();
            if let Some(failure) = self.capture_temporal_page(capture, request, entity_ids, forwarded, captured).await? {
                return Ok(Some(failure));
            }
            if !local_on && !pass.holds_pages() {
                return Ok(None);
            }
        }
    }

    /// Walk every source page again over the rest of the window past the cut
    /// of the previous pass reached least far, until a pass holds no partial
    /// answer.
    ///
    /// Each pass is read whole before it is stored, and a pass holding a source
    /// entity is held to the entity limit with every entity already captured.
    /// The sources that fail in a pass are recorded in `shortfall`.
    ///
    /// Returns the query's failure, or `None` once the sources answered the
    /// whole window.
    async fn capture_resumed_source_passes(
        &self,
        capture: &TemporalCapture<'_>,
        mut sources: SourceInstanceWalk,
        shortfall: &mut SourceShortfall,
        captured: &mut HashSet<Identifier>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let mut resumed: Option<AdmittedTemporalRequest> = None;
        loop {
            let interval = match sources.next_pass() {
                Ok(Some(interval)) => interval,
                Ok(None) => return Ok(None),
                Err(refusal) => return Ok(Some(capture_refused(capture, &ExecutionFailure::SourceWalkRefused(refusal)))),
            };
            // clone: each pass asks the sources the query as asked, over a resumed window
            let params = resumed.get_or_insert_with(|| capture.params.clone());
            params.set_interval(interval);
            let params: &AdmittedTemporalRequest = params;
            let mut pass = match self.gather_source_pass(capture, params).await {
                Ok(pass) => pass,
                Err(failure) => return Ok(Some(failure)),
            };
            shortfall.record(pass.warnings());
            let verdict = match self.judge_pass(capture, params, &pass, shortfall).await? {
                PassJudgement::Judged(verdict) => verdict,
                PassJudgement::Ended(failure) => return Ok(Some(failure)),
            };
            if pass.holds_entities() {
                let joining = pass.identifiers().into_iter().filter(|identifier| !captured.contains(*identifier)).count();
                if let Err(exceeded) = pass.admit_collection(captured.len().saturating_add(joining)) {
                    return Ok(Some(failure_detail(classify_result_budget_refusal(&exceeded).into_problem())));
                }
            }
            pass.settle(&verdict, &mut sources);
            let request = CapturePass {
                params,
                interval,
                verdict: &verdict,
            };
            while let Some(page) = pass.next_page() {
                let entity_ids = uncaptured_contributions(&verdict, &page, captured);
                if let Some(failure) = self.capture_temporal_page(capture, request, entity_ids, page, captured).await? {
                    return Ok(Some(failure));
                }
            }
        }
    }
}

/// The contributed entities `page` answers that the capture holds nothing of,
/// whose part held here a resumed pass reads over its own window.
///
/// An entity the capture holds was captured with its part held here, read
/// over a window holding this pass's, or the local read listed no part of it
/// there; an entity no source contributed to was decided by the local read.
fn uncaptured_contributions(verdict: &SplitVerdict, page: &[ForwardedTemporalEntities], captured: &HashSet<Identifier>) -> Vec<Identifier> {
    let uncaptured: HashSet<&Identifier> = page
        .iter()
        .flat_map(|answer| answer.entities.iter())
        .filter_map(TemporalHistory::id)
        .filter(|entity_id| verdict.contributed().contains(entity_id) && !captured.contains(*entity_id))
        .collect();
    uncaptured.into_iter().cloned().collect()
}

/// The query's failure when the capture cannot read the rest of its answer:
/// a partial answer it cannot read on, or pages past the offsets a source can
/// be asked for.
pub(super) fn capture_refused(capture: &TemporalCapture<'_>, failure: &ExecutionFailure<'_>) -> ExecutionResultDetails {
    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, %failure, "Snapshot temporal capture cannot read the rest of its answer");
    failure_detail(failure.problem())
}

#[cfg(test)]
mod tests {
    use crate::executor::{split_verdict::SplitVerdict, temporal_capture::uncaptured_contributions};
    use andromeda_federation::outcome::ForwardedTemporalEntities;
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_system_times::EntitySystemTimes,
        registration::forwarding_mode::ForwardingMode,
        temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_query::temporal::contributed_entities::{ContributedEntities, ContributedRendering};
    use andromeda_temporal::split_settlement::SplitSettlement;
    use indexmap::IndexMap;
    use std::collections::HashSet;

    fn identifier(number: u32) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{number}").parse().expect("a valid identifier")
    }

    fn answer(numbers: &[u32]) -> ForwardedTemporalEntities {
        let vehicle = |number: u32| {
            let document = EntityDocument::new(
                Some(identifier(number)),
                vec!["https://example.com/Vehicle".parse().expect("a type")],
                None,
                None,
                IndexMap::new(),
            );
            TemporalHistory::Instances(TemporalDocument::new(document, EntitySystemTimes::default(), IndexMap::new()))
        };
        ForwardedTemporalEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid identifier")),
            mode: ForwardingMode::Inclusive,
            entities: numbers.iter().copied().map(vehicle).collect(),
            warnings: Vec::new(),
            linked_map: None,
            partial_pages: Vec::new(),
        }
    }

    /// A resumed pass reads the part held here of a contributed entity the
    /// capture holds nothing of, once however many sources answered it; an
    /// entity it holds was captured with that part already.
    #[test]
    fn a_resumed_pass_reads_the_local_part_of_an_uncaptured_contribution_alone() {
        let verdict = SplitVerdict::new(
            ContributedEntities::new([identifier(1), identifier(2)], ContributedRendering::Local),
            SplitSettlement::default(),
        );
        let captured: HashSet<Identifier> = [identifier(1)].into_iter().collect();

        let read = uncaptured_contributions(&verdict, &[answer(&[1, 2]), answer(&[2])], &captured);

        pretty_assertions::assert_eq!(read, vec![identifier(2)]);
    }

    /// A pass whose holders decided their own filters reads no local part:
    /// the local read decided every local entity on the first pass.
    #[test]
    fn a_pass_of_holders_deciding_their_own_filters_reads_no_local_part() {
        let read = uncaptured_contributions(&SplitVerdict::default(), &[answer(&[1])], &HashSet::new());

        pretty_assertions::assert_eq!(read, Vec::<Identifier>::new());
    }
}
