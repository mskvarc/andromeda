//! Capturing the whole answer of one snapshot temporal query (clause
//! 5.16.1.4).
//!
//! The local entities are walked by keyset over their identifiers, so entities
//! written while the capture runs neither shift a later page nor appear twice
//! (clause 5.5.9.1); the context sources are paged by the offset their own
//! brokers understand. The instances of each page of local entities are then
//! read to the end of the window, page after page (clause 6.3.10). What the
//! sources cut short is read in further passes over every source page, each
//! over the window resumed past the previous pass.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        instance_walk::{InstanceWalk, WalkProgress, WalkRefusal},
        source_instance_walk::SourceInstanceWalk,
        status::{failure_detail, success_detail},
        temporal_failure::{TemporalStep, temporal_failure},
        translation::temporal_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    operation::ForwardingOperation,
    outcome::ForwardedTemporalEntities,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, SourcePage, SourcePaging, TriggeringRequest},
    temporal_result::merge_temporal_query_results,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    snapshot::ExecutionResultDetails,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
    temporal::{
        contributed_entities::ContributedEntities,
        window::{TemporalWindow, TimeInterval},
    },
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{
    error::TemporalError,
    query_reading::QueryReading,
    requested_identifiers::RequestedIdentifiers,
    service::{TemporalQueryResult, TemporalRange},
};
use std::collections::HashSet;
use tracing::{error, warn};

/// One snapshot temporal query, as every page of its capture reads it.
struct TemporalCapture<'q> {
    /// The Snapshot's staging branch.
    target: &'q ContextTarget,
    /// The Snapshot being prepared.
    snapshot_id: &'q Identifier,
    /// The query's position in `snapshotTemporalQueries`.
    index: usize,
    /// The query as the Snapshot asked it.
    params: &'q TemporalQueryParameters,
    /// The window the query asked for.
    interval: TimeInterval,
    /// The context the query is expanded under.
    context: &'q ContextSpec,
}

impl SnapshotExecutor {
    pub(crate) async fn run_temporal_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let params = match temporal_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        let interval = match TimeInterval::from_parts(params.timerel, params.time_at, params.end_time_at) {
            Ok(interval) => interval,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        let capture = TemporalCapture {
            target,
            snapshot_id,
            index,
            params: &params,
            interval,
            context,
        };
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        let mut source_offset = 0_u32;
        let mut sources = SourceInstanceWalk::new(interval, params.timeproperty_or_default(), params.last_n);
        let mut captured = HashSet::new();
        loop {
            let entity_ids = match self.local_entity_page(&capture, &window).await {
                Ok(entity_ids) => entity_ids,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal query failed");
                    return temporal_failure(error, TemporalStep::Query);
                }
            };
            let local_page_full = entity_ids.len() >= self.capture_page_size as usize;
            // clone: the cursor of the next page outlives the identifiers the page reads
            let last_local = entity_ids.last().cloned();
            let mut forwarded = self.forwarded_page(&capture, capture.params, source_offset).await;
            let remote_page_full = sources_page_on(&forwarded, self.capture_page_size);
            if let Err(refusal) = sources.trim(&mut forwarded) {
                return Ok(walk_refusal(&capture, refusal));
            }
            if let Some(failure) = self.capture_temporal_page(&capture, entity_ids, forwarded, &mut captured).await? {
                return Ok(failure);
            }
            if !local_page_full && !remote_page_full {
                break;
            }
            // A local walk that ran dry stays where it ended while the sources
            // still page on.
            if let Some(last) = last_local {
                window = PageWindow::after(self.capture_page_size, last);
            }
            source_offset = match source_offset.checked_add(self.capture_page_size) {
                Some(offset) => offset,
                None => return Ok(offset_overflow()),
            };
        }
        if let Some(failure) = self.capture_resumed_source_passes(&capture, sources, &mut captured).await? {
            return Ok(failure);
        }
        Ok(success_detail(captured.len()))
    }

    /// Walk every source page again over the window resumed past the least
    /// cut of the previous pass, until a pass holds no partial answer.
    ///
    /// Returns the query's failure, or `None` once the sources answered the
    /// whole window.
    async fn capture_resumed_source_passes(
        &self,
        capture: &TemporalCapture<'_>,
        mut sources: SourceInstanceWalk,
        captured: &mut HashSet<Identifier>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let mut resumed: Option<TemporalQueryParameters> = None;
        loop {
            let interval = match sources.next_pass() {
                Ok(Some(interval)) => interval,
                Ok(None) => return Ok(None),
                Err(refusal) => return Ok(Some(walk_refusal(capture, refusal))),
            };
            // clone: each pass asks the sources the query as asked, over a resumed window
            let params = resumed.get_or_insert_with(|| capture.params.clone());
            params.set_interval(interval);
            let mut offset = 0_u32;
            loop {
                let mut forwarded = self.forwarded_page(capture, params, offset).await;
                let page_on = sources_page_on(&forwarded, self.capture_page_size);
                if let Err(refusal) = sources.trim(&mut forwarded) {
                    return Ok(Some(walk_refusal(capture, refusal)));
                }
                if let Some(failure) = self.capture_temporal_page(capture, Vec::new(), forwarded, captured).await? {
                    return Ok(Some(failure));
                }
                if !page_on {
                    break;
                }
                offset = match offset.checked_add(self.capture_page_size) {
                    Some(offset) => offset,
                    None => return Ok(Some(offset_overflow())),
                };
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

    /// One offset page of what the registered context sources answer to
    /// `params`; a forwarding that failed leaves the capture with the local
    /// answer.
    async fn forwarded_page(&self, capture: &TemporalCapture<'_>, params: &TemporalQueryParameters, offset: u32) -> Vec<ForwardedTemporalEntities> {
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
                    csf_expr: None,
                    geo: None,
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                    paging: SourcePaging::Page(SourcePage {
                        limit: self.capture_page_size,
                        offset,
                    }),
                },
            )
            .await;
        match forwarded {
            Ok(forwarded) => forwarded.results,
            Err(error) => {
                warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal CSR forwarding failed");
                Vec::new()
            }
        }
    }

    /// Capture one page of entities: the first instances of the local ones
    /// merged with what the sources answered, then the rest of the local
    /// instances, until the window is exhausted.
    ///
    /// Returns the query's failure, or `None` once every instance of the page
    /// is stored.
    async fn capture_temporal_page(
        &self,
        capture: &TemporalCapture<'_>,
        entity_ids: Vec<Identifier>,
        forwarded: Vec<ForwardedTemporalEntities>,
        captured: &mut HashSet<Identifier>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let timeproperty = capture.params.timeproperty_or_default();
        let reads_local = !entity_ids.is_empty();
        // clone: the page's instances are read by a narrowed copy of the query while its identifier pages read the query as asked
        let mut narrowed = capture.params.clone();
        narrowed.select_only(entity_ids);
        let mut walk = InstanceWalk::new(capture.interval, timeproperty);
        let mut forwarded = Some(forwarded);
        loop {
            narrowed.set_interval(walk.interval());
            let (local, range) = if reads_local {
                match self.local_instances(&narrowed, capture).await {
                    Ok(page) => page,
                    Err(error) => {
                        warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal query failed");
                        return temporal_failure(error, TemporalStep::Query).map(Some);
                    }
                }
            } else {
                (Vec::new(), None)
            };
            let histories = match forwarded.take() {
                Some(forwarded) => merge_temporal_query_results(
                    local,
                    forwarded,
                    TemporalWindow::builder().interval(walk.interval()).timeproperty(timeproperty).build(),
                ),
                None => local,
            };
            let mut documents = match instance_documents(histories, capture) {
                Ok(documents) => documents,
                Err(failure) => return Ok(Some(failure)),
            };
            walk.trim(&mut documents);
            // clone: the query counts the entities after the page is stored
            captured.extend(documents.iter().filter_map(TemporalDocument::id).cloned());
            let detail = self.persist_temporal_results(capture, documents).await?;
            if detail.problem_details.is_some() {
                return Ok(Some(detail));
            }
            match walk.advance(range.as_ref()) {
                Ok(WalkProgress::Complete) => return Ok(None),
                Ok(WalkProgress::Resumed) => {}
                Err(refusal) => return Ok(Some(walk_refusal(capture, refusal))),
            }
        }
    }

    /// One page of the instances of the local entities `narrowed` lists, and
    /// the range a partial page ends at; no Context Source contributes to it.
    ///
    /// The read is boxed: it is the largest future of the capture loop, which
    /// would otherwise carry it inline across every await.
    async fn local_instances(
        &self,
        narrowed: &TemporalQueryParameters,
        capture: &TemporalCapture<'_>,
    ) -> Result<(Vec<TemporalHistory>, Option<TemporalRange>), TemporalError> {
        let window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        let TemporalQueryResult { entities, range, warnings, .. } = Box::pin(self.sources.temporal.query(
            &self.sources.live_target,
            narrowed,
            &window,
            CountPolicy::Omit,
            QueryReading {
                context: capture.context,
                language_targeting: self.sources.temporal.language_targeting(),
            },
            ContributedEntities::default(),
        ))
        .await?;
        for warning in &warnings {
            warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, entity_id = %warning.entity_id, warning = %warning.reason, "Snapshot temporal query retained usable results with a warning");
        }
        Ok((entities, range))
    }

    /// Write captured temporal evolutions into the Snapshot's branch.
    ///
    /// The evolutions arrive in the expanded form, whatever context the query
    /// was spelled in, so what is stored is what clause 5.16.1.4 says to store.
    async fn persist_temporal_results(&self, capture: &TemporalCapture<'_>, documents: Vec<TemporalDocument>) -> Result<ExecutionResultDetails, SnapshotError> {
        let stored = documents.len();
        for document in documents {
            if let Err(error) = self.sources.temporal.record_history(capture.target, document).await {
                error!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Failed to materialize temporal Snapshot results");
                return temporal_failure(error, TemporalStep::Store);
            }
        }
        Ok(success_detail(stored))
    }
}

/// The evolutions of `histories`, or the query's failure when one is an
/// aggregate: an aggregate records no instance, so there is no history of it
/// to capture.
fn instance_documents(histories: Vec<TemporalHistory>, capture: &TemporalCapture<'_>) -> Result<Vec<TemporalDocument>, ExecutionResultDetails> {
    let mut documents = Vec::with_capacity(histories.len());
    for history in histories {
        match history {
            TemporalHistory::Instances(document) if document.id().is_some() => documents.push(document),
            TemporalHistory::Instances(_) => {
                warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, "Temporal snapshot result carries no entity identifier");
            }
            TemporalHistory::Aggregates(_) => return Err(failure_detail("A Snapshot cannot capture an aggregated temporal representation")),
        }
    }
    Ok(documents)
}

/// Whether the sources may hold another offset page: one answered a full page,
/// or a partial one, which may leave out entities of a full page whose every
/// instance lies past its cut.
fn sources_page_on(forwarded: &[ForwardedTemporalEntities], page_size: u32) -> bool {
    forwarded
        .iter()
        .any(|source| source.entities.len() >= page_size as usize || !source.partial_ranges.is_empty())
}

/// The query's failure when the rest of a partial answer cannot be read.
fn walk_refusal(capture: &TemporalCapture<'_>, refusal: WalkRefusal) -> ExecutionResultDetails {
    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, %refusal, "Snapshot temporal capture cannot read the rest of a partial answer");
    failure_detail(refusal.to_string())
}

/// The query's failure when its pages run past the offsets a source can be
/// asked for.
fn offset_overflow() -> ExecutionResultDetails {
    failure_detail("Snapshot temporal pagination offset overflowed")
}
