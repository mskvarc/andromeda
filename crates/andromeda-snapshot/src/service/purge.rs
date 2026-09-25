//! Purging Snapshots by query (clause 5.16.7).

use crate::{error::SnapshotError, service::SnapshotService};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot_filter::SnapshotPurgeFilter;

impl SnapshotService {
    /// Purge snapshots matching a typed filter (ETSI GS CIM 009 clause 5.16.7).
    ///
    /// Matches candidate identifiers, then deletes each row individually so
    /// per-row failures can be surfaced in a 207 Multi-Status response
    /// (clause 6.36.3.2). Returns a [`PurgeOutcome`] describing the result:
    ///
    /// - [`PurgeOutcome::NoMatch`] when no snapshots match (maps to 404).
    /// - [`PurgeOutcome::FullSuccess`] when every matched row was deleted
    ///   (maps to 204).
    /// - [`PurgeOutcome::Partial`] when at least one row succeeded and at
    ///   least one failed (maps to 207).
    ///
    /// # Errors
    ///
    /// Returns the store's own failure when matching the filter does not
    /// complete. A per-row deletion failure is collected into the returned
    /// outcome instead of aborting the purge.
    pub async fn purge_by_query(&self, filter: &SnapshotPurgeFilter) -> Result<PurgeOutcome, SnapshotError> {
        let matched = self.repo.match_by_filter(filter).await?;
        if matched.is_empty() {
            return Ok(PurgeOutcome::NoMatch);
        }

        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for id in matched {
            match self.repo.delete_one(&id).await {
                Ok(n) if n > 0 => successes.push(id),
                Ok(_) => failures.push(SnapshotPurgeFailure {
                    error: SnapshotError::not_found(&id),
                    identifier: id,
                }),
                Err(error) => failures.push(SnapshotPurgeFailure { identifier: id, error }),
            }
        }

        if failures.is_empty() {
            Ok(PurgeOutcome::FullSuccess)
        } else if successes.is_empty() {
            Ok(PurgeOutcome::AllFailed { failures })
        } else {
            Ok(PurgeOutcome::Partial { successes, failures })
        }
    }
}

/// Outcome of [`SnapshotService::purge_by_query`] (ETSI GS CIM 009 clause 6.36.3.2).
#[derive(Debug)]
pub enum PurgeOutcome {
    /// No snapshots matched the filter (maps to 404 `ResourceNotFound`).
    NoMatch,
    /// Every matched snapshot was deleted (maps to 204 No Content).
    FullSuccess,
    /// All matched snapshots failed to delete (still 207 with only errors).
    AllFailed {
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<SnapshotPurgeFailure>,
    },
    /// Some succeeded and some failed (maps to 207 Multi-Status).
    Partial {
        /// Identifiers of successfully deleted snapshots.
        successes: Vec<Uri>,
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<SnapshotPurgeFailure>,
    },
}

/// One policy-owned failure produced while purging multiple snapshots.
#[derive(Debug)]
pub struct SnapshotPurgeFailure {
    /// Snapshot whose deletion failed.
    pub identifier: Uri,
    /// Focused reason for the failure.
    pub error: SnapshotError,
}
