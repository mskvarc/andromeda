//! The local entities a snapshot temporal query captures, page by page
//! (clauses 4.12, 5.5.9.1 and 5.16.1.4).
//!
//! The local entities are walked by keyset over their identifiers, so entities
//! written while the capture runs neither shift a later page nor appear twice
//! (clause 5.5.9.1). Once a source answered an entity, every identifier is
//! read before anything is stored, and the query is refused when they and the
//! sources' entities together pass the entity limit (clause 4.12).
//!
//! A split query's contributed entities are listed whatever its values
//! filter, geoquery and Scope query say of their part held here: their
//! gathered parts decide those filters (clause 5.7.4.4).

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        source_pass::SourcePass,
        status::failure_detail,
        temporal_capture::TemporalCapture,
        temporal_failure::{TemporalStep, temporal_failure},
    },
};
use andromeda_failure_classification::result_limit_failure::classify_result_budget_refusal;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    page_window::{PageStart, PageWindow},
    temporal::contributed_entities::ContributedEntities,
};
use andromeda_temporal::{candidate_listing::CandidateListing, error::TemporalError, query_reading::QueryReading, requested_identifiers::RequestedIdentifiers};
use std::vec::IntoIter;
use tracing::warn;

/// Where the local entities of a capture's pages come from.
pub(super) enum LocalEntities {
    /// Read page by page by keyset, the next page starting where the window
    /// says.
    Keyset(PageWindow),
    /// Every one, read while the merged collection was admitted.
    Admitted(IntoIter<Identifier>),
}

impl LocalEntities {
    /// The keyset walk of every local entity, from the first.
    pub(super) const fn from_first(page_size: u32) -> LocalEntities {
        LocalEntities::Keyset(PageWindow {
            limit: page_size,
            start: PageStart::Cursor(None),
        })
    }

    /// Whether another local page may follow `page`, the one just taken.
    pub(super) fn continues_after(&self, page: &[Identifier], page_size: u32) -> bool {
        match self {
            LocalEntities::Keyset(_) => page.len() >= page_size as usize,
            LocalEntities::Admitted(rest) => !rest.as_slice().is_empty(),
        }
    }
}

/// What reading every local entity of a capture whose sources answered an
/// entity came to.
pub(super) enum LocalAdmission {
    /// Every local entity, the merged collection within the entity limit.
    Admitted(Vec<Identifier>),
    /// The query's failure: the merged collection passed the entity limit, or
    /// a local page could not be read.
    Ended(ExecutionResultDetails),
}

impl SnapshotExecutor {
    /// Every local entity of the query, read before anything is stored once
    /// `pass` holds a source entity, and counted with the pass's entities
    /// against the entity limit, each entity once; the filters do not hold
    /// `contributed`.
    ///
    /// # Errors
    ///
    /// Returns a transient failure a local page read met.
    pub(super) async fn admitted_local_entities(
        &self,
        capture: &TemporalCapture<'_>,
        pass: &SourcePass,
        contributed: &ContributedEntities,
    ) -> Result<LocalAdmission, SnapshotError> {
        let mut unclaimed = pass.identifiers();
        let mut admitted = Vec::new();
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        loop {
            let page = match self.local_entity_page(capture, &window, contributed).await {
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

    /// The identifiers of the next page of local entities; the filters do not
    /// hold `contributed`.
    pub(super) async fn next_local_page(
        &self,
        capture: &TemporalCapture<'_>,
        local: &mut LocalEntities,
        contributed: &ContributedEntities,
    ) -> Result<Vec<Identifier>, TemporalError> {
        match local {
            LocalEntities::Keyset(window) => {
                let page = self.local_entity_page(capture, window, contributed).await?;
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

    /// The identifiers of one keyset page of the local entities the query
    /// selects; the filters do not hold `contributed`.
    ///
    /// The page is the identifiers the query matched, not the entities a
    /// partial answer rendered: an entity whose instances in the window all lie
    /// past the answer's cut is left out of that answer, yet belongs to the page.
    async fn local_entity_page(
        &self,
        capture: &TemporalCapture<'_>,
        window: &PageWindow,
        contributed: &ContributedEntities,
    ) -> Result<Vec<Identifier>, TemporalError> {
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
                CandidateListing {
                    identifiers: RequestedIdentifiers::Narrow,
                    filters: CandidateFilters::Applied,
                    // clone: the listing's compiled query owns the exemption every page of the walk is read under
                    contributed: contributed.clone(),
                },
            )
            .await
    }
}
