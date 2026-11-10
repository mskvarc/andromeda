//! Capturing the whole answer of one snapshot temporal query (clause
//! 5.16.1.4).
//!
//! The context sources are asked first: every offset page of the window the
//! query asked for is read whole, charged to one budget as Query Temporal
//! Evolution of Entities charges the sources of one request (clause 4.12). The
//! local entities are walked by keyset over their identifiers, so entities
//! written while the capture runs neither shift a later page nor appear twice
//! (clause 5.5.9.1), and each local page is stored beside the source page read
//! in the same position. The instances of each page of local entities are then
//! read to the end of the window, page after page (clause 6.3.10). What the
//! sources cut short is read in further passes over every source page, each
//! over the window resumed past the previous pass.
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
        source_pass::SourcePass,
        source_shortfall::SourceShortfall,
        status::{answered_detail, failure_detail},
        temporal_failure::{TemporalStep, temporal_failure},
        translation::temporal_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_failure_classification::{
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
    result_limit_failure::classify_result_budget_refusal,
};
use andromeda_federation::expanded_registration_filters::ExpandedRegistrationFilters;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{query::body::Query, snapshot::ExecutionResultDetails};
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    error::QueryError,
    page_window::{PageStart, PageWindow},
    temporal::window::TimeInterval,
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{error::TemporalError, query_reading::QueryReading, requested_identifiers::RequestedIdentifiers};
use std::{collections::HashSet, vec::IntoIter};
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
    pub(super) params: &'q TemporalQueryParameters,
    /// The window the query asked for.
    pub(super) interval: TimeInterval,
    /// The context the query is expanded under.
    pub(super) context: &'q ContextSpec,
    /// The query's context source filter and geoquery, which select the
    /// registrations it reaches.
    pub(super) filters: &'q ExpandedRegistrationFilters,
}

/// Where the local entities of a capture's pages come from.
enum LocalEntities {
    /// Read page by page by keyset, the next page starting where the window
    /// says.
    Keyset(PageWindow),
    /// Every one, read while the merged collection was admitted.
    Admitted(IntoIter<Identifier>),
}

impl LocalEntities {
    /// Whether another local page may follow `page`, the one just taken.
    fn continues_after(&self, page: &[Identifier], page_size: u32) -> bool {
        match self {
            LocalEntities::Keyset(_) => page.len() >= page_size as usize,
            LocalEntities::Admitted(rest) => !rest.as_slice().is_empty(),
        }
    }
}

/// What reading every local entity of a capture whose sources answered an
/// entity came to.
enum LocalAdmission {
    /// Every local entity, the merged collection within the entity limit.
    Admitted(Vec<Identifier>),
    /// The query's failure: the merged collection passed the entity limit, or
    /// a local page could not be read.
    Ended(ExecutionResultDetails),
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
            Err(error) => return Ok(failure_detail(classify_query_error(&error).into_problem())),
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
        };
        let mut sources = SourceInstanceWalk::new(interval, params.timeproperty_or_default(), params.last_n);
        let mut first_pass = match self.gather_source_pass(&capture, capture.params, &mut sources).await {
            Ok(pass) => pass,
            Err(failure) => return Ok(failure),
        };
        let mut shortfall = SourceShortfall::default();
        shortfall.record(first_pass.warnings());
        let mut local = if first_pass.holds_entities() {
            match self.admitted_local_entities(&capture, &first_pass).await? {
                LocalAdmission::Admitted(identifiers) => LocalEntities::Admitted(identifiers.into_iter()),
                LocalAdmission::Ended(failure) => return Ok(failure),
            }
        } else {
            LocalEntities::Keyset(PageWindow {
                limit: self.capture_page_size,
                start: PageStart::Cursor(None),
            })
        };
        loop {
            let entity_ids = match self.next_local_page(&capture, &mut local).await {
                Ok(entity_ids) => entity_ids,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal query failed");
                    return temporal_failure(error, TemporalStep::Query);
                }
            };
            let local_on = local.continues_after(&entity_ids, self.capture_page_size);
            let forwarded = first_pass.next_page().unwrap_or_default();
            if let Some(failure) = self.capture_temporal_page(&capture, entity_ids, forwarded, captured).await? {
                return Ok(failure);
            }
            if !local_on && !first_pass.holds_pages() {
                break;
            }
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

    /// Every local entity of the query, read before anything is stored once
    /// `pass` holds a source entity, and counted with the pass's entities
    /// against the entity limit, each entity once.
    ///
    /// # Errors
    ///
    /// Returns a transient failure a local page read met.
    async fn admitted_local_entities(&self, capture: &TemporalCapture<'_>, pass: &SourcePass) -> Result<LocalAdmission, SnapshotError> {
        let mut unclaimed = pass.identifiers();
        let mut admitted = Vec::new();
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        loop {
            let page = match self.local_entity_page(capture, &window).await {
                Ok(page) => page,
                Err(error) => {
                    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal query failed");
                    return temporal_failure(error, TemporalStep::Query).map(LocalAdmission::Ended);
                }
            };
            let page_full = page.len() >= self.capture_page_size as usize;
            for identifier in &page {
                unclaimed.remove(identifier);
            }
            admitted.extend(page);
            if let Err(exceeded) = pass.admit_collection(admitted.len().saturating_add(unclaimed.len())) {
                return Ok(LocalAdmission::Ended(failure_detail(classify_result_budget_refusal(&exceeded).into_problem())));
            }
            match admitted.last() {
                // clone: the cursor of the next page outlives the identifiers admitted so far
                Some(last) if page_full => window = PageWindow::after(self.capture_page_size, last.clone()),
                Some(_) | None => return Ok(LocalAdmission::Admitted(admitted)),
            }
        }
    }

    /// The identifiers of the next page of local entities.
    async fn next_local_page(&self, capture: &TemporalCapture<'_>, local: &mut LocalEntities) -> Result<Vec<Identifier>, TemporalError> {
        match local {
            LocalEntities::Keyset(window) => {
                let page = self.local_entity_page(capture, window).await?;
                // A local walk that ran dry stays where it ended while the
                // sources still page on.
                if let Some(last) = page.last() {
                    // clone: the cursor of the next page outlives the identifiers the page reads
                    *window = PageWindow::after(self.capture_page_size, last.clone());
                }
                Ok(page)
            }
            LocalEntities::Admitted(rest) => Ok(rest.by_ref().take(self.capture_page_size as usize).collect()),
        }
    }

    /// Walk every source page again over the window resumed past the least
    /// cut of the previous pass, until a pass holds no partial answer.
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
        let mut resumed: Option<TemporalQueryParameters> = None;
        loop {
            let interval = match sources.next_pass() {
                Ok(Some(interval)) => interval,
                Ok(None) => return Ok(None),
                Err(refusal) => return Ok(Some(capture_refused(capture, &ExecutionFailure::SourceWalkRefused(refusal)))),
            };
            // clone: each pass asks the sources the query as asked, over a resumed window
            let params = resumed.get_or_insert_with(|| capture.params.clone());
            params.set_interval(interval);
            let mut pass = match self.gather_source_pass(capture, params, &mut sources).await {
                Ok(pass) => pass,
                Err(failure) => return Ok(Some(failure)),
            };
            shortfall.record(pass.warnings());
            if pass.holds_entities() {
                let joining = pass.identifiers().into_iter().filter(|identifier| !captured.contains(*identifier)).count();
                if let Err(exceeded) = pass.admit_collection(captured.len().saturating_add(joining)) {
                    return Ok(Some(failure_detail(classify_result_budget_refusal(&exceeded).into_problem())));
                }
            }
            while let Some(page) = pass.next_page() {
                if let Some(failure) = self.capture_temporal_page(capture, Vec::new(), page, captured).await? {
                    return Ok(Some(failure));
                }
            }
        }
    }

    /// The identifiers of one keyset page of the local entities the query
    /// selects.
    ///
    /// The page is the identifiers the query matched, not the entities a
    /// partial answer rendered: an entity whose instances in the window all lie
    /// past the answer's cut is left out of that answer, yet belongs to the page.
    async fn local_entity_page(&self, capture: &TemporalCapture<'_>, window: &PageWindow) -> Result<Vec<Identifier>, TemporalError> {
        self.sources
            .temporal
            .query_ids(
                &self.sources.live_target,
                capture.params,
                window,
                QueryReading {
                    context: capture.context,
                    language_targeting: self.sources.temporal.language_targeting(),
                },
                RequestedIdentifiers::Narrow,
                CandidateFilters::Applied,
            )
            .await
    }
}

/// The query's failure when the capture cannot read the rest of its answer:
/// a partial answer it cannot read on, or pages past the offsets a source can
/// be asked for.
pub(super) fn capture_refused(capture: &TemporalCapture<'_>, failure: &ExecutionFailure<'_>) -> ExecutionResultDetails {
    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, %failure, "Snapshot temporal capture cannot read the rest of its answer");
    failure_detail(failure.problem())
}
