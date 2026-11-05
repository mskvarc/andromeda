//! Accounting for the committed changes a stopping tenant's drain gives up on.
//!
//! The drain runs the changes a tenant still holds through evaluation before
//! its worker returns. When the shutdown grace runs out the worker is aborted
//! mid-drain, and what it still holds is lost. Dropping an unfinished
//! [`EntityDrain`] is the one place that is seen, because an abort drops the
//! worker's future and with it the guard, while the lanes it feeds are still
//! alive.

use andromeda_notification::pending_change::PendingCommits;
use andromeda_telemetry::live_evaluation::record_abandoned_changes;
use std::sync::Arc;
use tracing::warn;

/// A drain of held entity changes that has begun and not finished.
pub(super) struct EntityDrain {
    /// The changes held, until the drain finishes; `None` once it has.
    unfinished: Option<Arc<PendingCommits>>,
}

impl EntityDrain {
    /// Begin a drain of the changes `commits` holds.
    pub(super) fn begin(commits: &Arc<PendingCommits>) -> EntityDrain {
        EntityDrain {
            unfinished: Some(Arc::clone(commits)), // clone: the guard reads the count when it is dropped unfinished.
        }
    }

    /// Mark the drain finished, so dropping the guard reports nothing.
    pub(super) fn finish(mut self) {
        self.unfinished = None;
    }
}

impl Drop for EntityDrain {
    fn drop(&mut self) {
        let Some(commits) = self.unfinished.take() else {
            return;
        };
        let abandoned = commits.count();
        if abandoned > 0 {
            warn!(
                abandoned,
                "stopping tenant gave up evaluating committed changes: no notification is sent for them"
            );
            record_abandoned_changes(abandoned);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::{held_change_fixture::committed_change, tasks::abandoned_drain::EntityDrain};
    use andromeda_notification::pending_change::PendingCommits;
    use andromeda_telemetry::{error::TelemetryError, names::LIVE_CHANGES_ABANDONED_TOTAL, recorder::TelemetryRecorder};
    use std::sync::Arc;

    #[test]
    fn a_drain_dropped_before_it_finishes_counts_the_changes_it_still_held() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        let commits = Arc::new(PendingCommits::default());
        let _held = [
            commits.hold(committed_change()),
            commits.hold(committed_change()),
            commits.hold(committed_change()),
        ];
        recorder.record_locally(|| drop(EntityDrain::begin(&commits)));
        assert!(
            recorder.render().contains(&format!("{LIVE_CHANGES_ABANDONED_TOTAL} 3")),
            "{}",
            recorder.render()
        );
        Ok(())
    }

    #[test]
    fn a_finished_drain_counts_nothing() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        let commits = Arc::new(PendingCommits::default());
        let _held = commits.hold(committed_change());
        recorder.record_locally(|| EntityDrain::begin(&commits).finish());
        assert!(!recorder.render().contains(LIVE_CHANGES_ABANDONED_TOTAL), "{}", recorder.render());
        Ok(())
    }
}
