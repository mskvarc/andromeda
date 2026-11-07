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
        instance_walk::WalkRefusal,
        source_instance_walk::SourceInstanceWalk,
        status::{failure_detail, success_detail},
        temporal_failure::{TemporalStep, temporal_failure},
        translation::temporal_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    expanded_registration_filters::ExpandedRegistrationFilters,
    operation::ForwardingOperation,
    outcome::ForwardedTemporalEntities,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, SourcePage, SourcePaging, TriggeringRequest},
    source_reach::SourceReach,
};
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
    pub(super) params: &'q TemporalQueryParameters,
    /// The window the query asked for.
    pub(super) interval: TimeInterval,
    /// The context the query is expanded under.
    pub(super) context: &'q ContextSpec,
    /// The query's context source filter and geoquery, which select the
    /// registrations it reaches.
    pub(super) filters: &'q ExpandedRegistrationFilters,
}

impl SnapshotExecutor {
    /// Execute one snapshot temporal query and capture its whole result,
    /// paging the local history and the forwarded sources side by side.
    ///
    /// # Errors
    ///
    /// Returns a transient failure a local page read or a history write met,
    /// on which the preparation restarts; the query's own failures are
    /// recorded in the returned details.
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
        let filters = match self.temporal_registration_filters(&params, context).await {
            Ok(filters) => filters,
            Err(error) => return Ok(failure_detail(error.to_string())),
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
                    csf_expr: capture.filters.csf(),
                    geo: capture.filters.geo(),
                    mode_filter: CsrModeFilter::All,
                    reach: SourceReach::Every,
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
pub(super) fn walk_refusal(capture: &TemporalCapture<'_>, refusal: WalkRefusal) -> ExecutionResultDetails {
    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, %refusal, "Snapshot temporal capture cannot read the rest of a partial answer");
    failure_detail(refusal.to_string())
}

/// The query's failure when its pages run past the offsets a source can be
/// asked for.
fn offset_overflow() -> ExecutionResultDetails {
    failure_detail("Snapshot temporal pagination offset overflowed")
}
