//! The earliest work a tenant has due across every store that owes it some.
//!
//! Each store answers for the work it owns: the context store for its queues,
//! expiries and periodic notifications, the temporal store for the history its
//! sweeps convert and age out. The wake loop needs one instant, so the sources
//! are asked in order and their answers folded into the earliest.
//!
//! A source that finds the database gone or its schema not migrated ends the
//! probe: the tenant is cleared or left to its standing whatever the others
//! would say, and a later source is never sent to read a schema an earlier one
//! refused. A known due instant outranks a source that could not answer, since
//! the tenant loaded for it runs every sweep, the unanswered source's included.
//! Only when no source has work due does a backpressure or fault answer reach
//! the wake loop, which retries it.

use crate::pending_work::{PendingWork, PendingWorkError, PendingWorkProbe};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use async_trait::async_trait;
use std::{sync::Arc, time::SystemTime};

/// Every source of a tenant's pending work, asked in order.
pub struct PendingWorkSources(Vec<Arc<dyn PendingWorkProbe>>);

impl PendingWorkSources {
    /// The sources in the order they are asked. The first is the one that
    /// vouches for the tenant's schema before any other reads it.
    #[must_use]
    pub fn new(sources: Vec<Arc<dyn PendingWorkProbe>>) -> PendingWorkSources {
        PendingWorkSources(sources)
    }
}

/// What the sources asked so far have answered.
enum Folded {
    /// Nothing is due and every source answered.
    Nothing,
    /// The earliest due instant any source answered.
    Due(SystemTime),
    /// No source has work due and one could not answer.
    Unanswered(PendingWorkError),
}

impl Folded {
    /// Fold one more source's answer in, `Err` when it ends the probe.
    fn with(self, answer: Result<PendingWork, PendingWorkError>) -> Result<Folded, PendingWorkError> {
        match (self, answer) {
            (_, Err(ended @ (PendingWorkError::DatabaseGone { .. } | PendingWorkError::Unmigrated { .. }))) => Err(ended),
            (Folded::Due(held), Ok(PendingWork::DueAt(due))) => Ok(Folded::Due(held.min(due))),
            (Folded::Nothing | Folded::Unanswered(_), Ok(PendingWork::DueAt(due))) => Ok(Folded::Due(due)),
            (due @ Folded::Due(_), Ok(PendingWork::None) | Err(PendingWorkError::Saturated { .. } | PendingWorkError::ProbeFailed { .. })) => Ok(due),
            (Folded::Nothing, Err(unanswered @ (PendingWorkError::Saturated { .. } | PendingWorkError::ProbeFailed { .. }))) => {
                Ok(Folded::Unanswered(unanswered))
            }
            (folded @ (Folded::Nothing | Folded::Unanswered(_)), Ok(PendingWork::None))
            | (folded @ Folded::Unanswered(_), Err(PendingWorkError::Saturated { .. } | PendingWorkError::ProbeFailed { .. })) => Ok(folded),
        }
    }

    /// The answer the wake loop receives.
    fn answer(self) -> Result<PendingWork, PendingWorkError> {
        match self {
            Folded::Nothing => Ok(PendingWork::None),
            Folded::Due(due) => Ok(PendingWork::DueAt(due)),
            Folded::Unanswered(error) => Err(error),
        }
    }
}

#[async_trait]
impl PendingWorkProbe for PendingWorkSources {
    async fn earliest_pending_work(&self, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
        let mut folded = Folded::Nothing;
        for source in &self.0 {
            folded = folded.with(source.earliest_pending_work(database).await)?;
        }
        folded.answer()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
        pending_work_sources::PendingWorkSources,
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime},
    };

    /// The answer a fake source gives.
    #[derive(Clone, Copy)]
    enum Answer {
        /// Nothing due.
        Nothing,
        /// Due this many seconds after the epoch.
        DueAt(u64),
        /// The probe connection was saturated.
        Saturated,
        /// The read failed.
        Failed,
        /// The schema is not migrated.
        Unmigrated,
        /// The database is gone.
        Gone,
    }

    /// A source answering `answer` and counting how often it was asked.
    struct Source {
        /// What it answers.
        answer: Answer,
        /// How often it was asked.
        asked: AtomicUsize,
    }

    #[async_trait]
    impl PendingWorkProbe for Source {
        async fn earliest_pending_work(&self, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let database = database.clone(); // clone: the error outlives this borrow
            match self.answer {
                Answer::Nothing => Ok(PendingWork::None),
                Answer::DueAt(seconds) => Ok(PendingWork::DueAt(at(seconds))),
                Answer::Saturated => Err(PendingWorkError::Saturated { database }),
                Answer::Failed => Err(PendingWorkError::ProbeFailed {
                    database,
                    cause: PersistenceFailure::ContractBroken,
                }),
                Answer::Unmigrated => Err(PendingWorkError::Unmigrated { database }),
                Answer::Gone => Err(PendingWorkError::DatabaseGone { database }),
            }
        }
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn source(answer: Answer) -> Arc<Source> {
        Arc::new(Source {
            answer,
            asked: AtomicUsize::new(0),
        })
    }

    async fn probe(sources: &[Arc<Source>]) -> Result<PendingWork, PendingWorkError> {
        PendingWorkSources::new(sources.iter().map(|source| Arc::clone(source) as Arc<dyn PendingWorkProbe>).collect())
            .earliest_pending_work(&TenantDatabaseName::from("andromeda_tenant_t1".to_owned()))
            .await
    }

    #[tokio::test]
    async fn the_earliest_due_instant_of_every_source_is_the_tenant_s() {
        let answer = probe(&[source(Answer::DueAt(500)), source(Answer::Nothing), source(Answer::DueAt(200))]).await;
        assert_eq!(answer.ok(), Some(PendingWork::DueAt(at(200))));
    }

    #[tokio::test]
    async fn no_source_with_work_due_leaves_the_tenant_with_none() {
        assert_eq!(probe(&[source(Answer::Nothing), source(Answer::Nothing)]).await.ok(), Some(PendingWork::None));
        assert_eq!(probe(&[]).await.ok(), Some(PendingWork::None));
    }

    #[tokio::test]
    async fn a_gone_database_or_unmigrated_schema_ends_the_probe_before_the_next_source() {
        let later = source(Answer::DueAt(100));
        assert!(matches!(
            probe(&[source(Answer::Unmigrated), Arc::clone(&later)]).await,
            Err(PendingWorkError::Unmigrated { .. })
        ));
        assert!(matches!(
            probe(&[source(Answer::Gone), Arc::clone(&later)]).await,
            Err(PendingWorkError::DatabaseGone { .. })
        ));
        assert_eq!(later.asked.load(Ordering::SeqCst), 0);
        assert!(matches!(
            probe(&[source(Answer::DueAt(100)), source(Answer::Gone)]).await,
            Err(PendingWorkError::DatabaseGone { .. })
        ));
    }

    #[tokio::test]
    async fn a_known_due_instant_outranks_a_source_that_could_not_answer() {
        assert_eq!(
            probe(&[source(Answer::DueAt(300)), source(Answer::Failed)]).await.ok(),
            Some(PendingWork::DueAt(at(300)))
        );
        assert_eq!(
            probe(&[source(Answer::Saturated), source(Answer::DueAt(300))]).await.ok(),
            Some(PendingWork::DueAt(at(300)))
        );
    }

    #[tokio::test]
    async fn a_source_that_could_not_answer_is_retried_when_nothing_else_is_due() {
        assert!(matches!(
            probe(&[source(Answer::Nothing), source(Answer::Saturated)]).await,
            Err(PendingWorkError::Saturated { .. })
        ));
        assert!(matches!(
            probe(&[source(Answer::Failed), source(Answer::Saturated)]).await,
            Err(PendingWorkError::ProbeFailed { .. })
        ));
    }
}
