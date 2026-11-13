//! Resolving the Snapshot a request is scoped to (clause 5.5.15).

use crate::{
    error::SnapshotError,
    service::SnapshotService,
    status::{ScopingEligibility, scoping_eligibility},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use chrono::Utc;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval, timeout};

impl SnapshotService {
    /// Validate a snapshot for header-based scoping (ETSI GS CIM 009 clause 5.5.15).
    ///
    /// The snapshot must exist, not be expired, and have a queryable status:
    /// `Success`, `Partial`, or `Empty` (clause 5.2.40: an empty snapshot is a
    /// prepared one whose queries matched nothing, so it scopes to an empty
    /// dataset rather than to "no snapshot"). Touches `last_used_at` on success.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::NotFound`] when the snapshot is absent or
    /// expired, [`SnapshotError::PreparationTimeout`] when it is still
    /// `preparing` at the scoped-operation deadline, and the store's own failure
    /// when the read does not complete.
    pub async fn validate_for_scoping(&self, id: &Identifier) -> Result<Snapshot, SnapshotError> {
        let snapshot = self.repo.get(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;

        if snapshot.expires_at < Utc::now() {
            if let Err(e) = self.repo.delete(id).await {
                tracing::error!(error = %e, snapshot_id = %id, "Failed to delete expired snapshot");
            }
            return Err(SnapshotError::not_found(id));
        }

        let snapshot = match scoping_eligibility(snapshot.status) {
            ScopingEligibility::Usable => snapshot,
            ScopingEligibility::Preparing => self.wait_for_terminal(id).await?,
        };

        if let Err(e) = self.repo.touch_last_used(id).await {
            tracing::warn!(error = %e, snapshot_id = %id, "Failed to update snapshot last_used_at");
        }

        Ok(snapshot)
    }

    /// Resolve and validate the typed branch target for a scoped operation.
    ///
    /// # Errors
    ///
    /// Returns every failure of [`SnapshotService::validate_for_scoping`], plus
    /// [`SnapshotError::NotFound`] when the validated snapshot owns no durable
    /// context branch.
    pub async fn resolve_context_target(&self, id: &Identifier) -> Result<ContextTarget, SnapshotError> {
        self.validate_for_scoping(id).await?;
        let target = self.repo.target(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;
        Ok(ContextTarget::Snapshot(target))
    }

    /// Wait until capture or clone promotion leaves `preparing` without treating status as absence.
    async fn wait_for_terminal(&self, id: &Identifier) -> Result<Snapshot, SnapshotError> {
        let wait = async {
            let mut poll = interval(Duration::from_millis(25));
            poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                poll.tick().await;
                let snapshot = self.repo.get(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;
                if snapshot.expires_at < Utc::now() {
                    return Err(SnapshotError::not_found(id));
                }
                match scoping_eligibility(snapshot.status) {
                    ScopingEligibility::Usable => return Ok(snapshot),
                    ScopingEligibility::Preparing => {}
                }
            }
        };
        timeout(self.config.scoped_operation_wait, wait)
            .await
            .map_err(|_| SnapshotError::preparation_timeout(id))?
    }
}
