//! Capturing one page of a snapshot temporal query's entities (clause
//! 5.16.1.4).
//!
//! The first instances of the page's local entities are merged with what the
//! sources answered for the page, and the rest of the local instances are then
//! read to the end of the window, page after page (clause 6.3.10), each stored
//! in the Snapshot's branch as it is read.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        instance_walk::{InstanceWalk, WalkProgress},
        status::{failure_detail, success_detail},
        temporal_capture::{TemporalCapture, walk_refusal},
        temporal_failure::{TemporalStep, temporal_failure},
    },
};
use andromeda_federation::{outcome::ForwardedTemporalEntities, temporal_result::merge_temporal_query_results};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    snapshot::ExecutionResultDetails,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
    temporal::{contributed_entities::ContributedEntities, window::TemporalWindow},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{
    error::TemporalError,
    query_reading::QueryReading,
    service::{TemporalQueryResult, TemporalRange},
};
use std::collections::HashSet;
use tracing::{error, warn};

impl SnapshotExecutor {
    /// Capture one page of entities: the first instances of the local ones
    /// merged with what the sources answered, then the rest of the local
    /// instances, until the window is exhausted.
    ///
    /// Returns the query's failure, or `None` once every instance of the page
    /// is stored.
    pub(super) async fn capture_temporal_page(
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
