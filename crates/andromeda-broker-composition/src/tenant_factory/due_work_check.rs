//! Whether a resident tenant's due work has run, for a cache with every place
//! taken that asks the runtimes it loaded for due work.
//!
//! The answer is the one obligation read the wake loop and the background
//! pacer use, over the tenant's own pool: work is outstanding while anything
//! the tenant owes is due now, while a change it accepted still waits for live
//! evaluation, and whenever the read fails.

use crate::tenant_factory::background_pacer::ObligationSource;
use andromeda_notification::pending_change::PendingCommits;
use andromeda_tenancy::due_work::{DueWork, DueWorkSource};
use async_trait::async_trait;
use std::{sync::Arc, time::SystemTime};
use tracing::warn;

/// A tenant's due work, read from its obligations and its pending commits.
pub(super) struct TenantDueWork {
    /// Where the tenant's earliest obligation is read.
    obligations: Arc<dyn ObligationSource>,
    /// The committed changes handed to live evaluation and not yet evaluated.
    commits: Arc<PendingCommits>,
}

impl TenantDueWork {
    /// The due work of a tenant whose obligations `obligations` reads and
    /// whose pending changes `commits` holds.
    pub(super) const fn new(obligations: Arc<dyn ObligationSource>, commits: Arc<PendingCommits>) -> TenantDueWork {
        TenantDueWork { obligations, commits }
    }
}

#[async_trait]
impl DueWorkSource for TenantDueWork {
    async fn due_work(&self) -> DueWork {
        if self.commits.count() > 0 {
            return DueWork::Outstanding;
        }
        match self.obligations.earliest_due().await {
            Ok(Some(due)) if due <= SystemTime::now() => DueWork::Outstanding,
            Ok(Some(_) | None) => DueWork::Drained,
            Err(error) => {
                warn!(%error, event = "tenant_due_work_read_failed");
                DueWork::Outstanding
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::{background_pacer::ObligationSource, due_work_check::TenantDueWork, held_change_fixture::committed_change};
    use andromeda_notification::pending_change::PendingCommits;
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_tenancy::due_work::{DueWork, DueWorkSource};
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use std::{
        sync::Arc,
        time::{Duration, SystemTime},
    };

    /// What the scripted obligations answer.
    #[derive(Clone, Copy)]
    enum Earliest {
        /// Due this long ago.
        Past(Duration),
        /// Due this long ahead.
        Ahead(Duration),
        /// Nothing outstanding.
        Nothing,
        /// The read fails.
        Failing,
    }

    /// Obligations answering one scripted earliest instant.
    struct Scripted(Earliest);

    #[async_trait]
    impl ObligationSource for Scripted {
        async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
            match self.0 {
                Earliest::Past(ago) => Ok(Some(SystemTime::now() - ago)),
                Earliest::Ahead(ahead) => Ok(Some(SystemTime::now() + ahead)),
                Earliest::Nothing => Ok(None),
                Earliest::Failing => Err(ContextStoreError::Database(sqlx::Error::RowNotFound)),
            }
        }
    }

    fn due_work(earliest: Earliest, commits: &Arc<PendingCommits>) -> TenantDueWork {
        TenantDueWork::new(Arc::new(Scripted(earliest)), Arc::clone(commits))
    }

    #[tokio::test]
    async fn work_due_now_is_outstanding() {
        let commits = Arc::new(PendingCommits::default());
        assert_eq!(
            due_work(Earliest::Past(Duration::from_secs(1)), &commits).due_work().await,
            DueWork::Outstanding
        );
    }

    #[tokio::test]
    async fn work_due_only_ahead_or_none_at_all_is_drained() {
        let commits = Arc::new(PendingCommits::default());
        assert_eq!(due_work(Earliest::Ahead(Duration::from_secs(30)), &commits).due_work().await, DueWork::Drained);
        assert_eq!(due_work(Earliest::Nothing, &commits).due_work().await, DueWork::Drained);
    }

    #[tokio::test]
    async fn a_change_still_waiting_for_live_evaluation_is_outstanding_whatever_the_obligations() {
        let commits = Arc::new(PendingCommits::default());
        let held = commits.hold(committed_change());
        assert_eq!(due_work(Earliest::Nothing, &commits).due_work().await, DueWork::Outstanding);
        drop(held);
        assert_eq!(due_work(Earliest::Nothing, &commits).due_work().await, DueWork::Drained);
    }

    #[tokio::test]
    async fn an_obligation_read_that_fails_is_outstanding() {
        let commits = Arc::new(PendingCommits::default());
        assert_eq!(due_work(Earliest::Failing, &commits).due_work().await, DueWork::Outstanding);
    }
}
