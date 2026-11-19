//! One attempt of a compare-and-set write, and the run that repeats it while
//! a concurrent writer keeps superseding it.

use crate::{EntityService, error::EntityError};
use andromeda_context_commit::write_retry::{WRITE_ATTEMPTS, WRITE_RETRY};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_retry::{retry_number::RetryNumber, retry_run::RetryRun};
use derive_more::{From, IsVariant};

/// Why one attempt of a compare-and-set write did not settle.
#[derive(Debug, From, IsVariant)]
pub(crate) enum ContendedAttempt {
    /// A concurrent writer superseded the state the attempt was computed
    /// from, so the next attempt reads it again.
    #[from(ignore)]
    Lost,
    /// The attempt failed for a reason another attempt cannot clear.
    Failed(EntityError),
}

impl ContendedAttempt {
    /// Whether the attempt runs again, which only a lost one does.
    pub(crate) const fn retried(&self, _retry: RetryNumber) -> bool {
        self.is_lost()
    }

    /// The error a write to `id` reports: a lost last attempt is contention
    /// over the whole budget.
    pub(crate) fn into_entity_error(self, id: &Identifier) -> EntityError {
        match self {
            ContendedAttempt::Lost => EntityError::contention(id, WRITE_ATTEMPTS),
            ContendedAttempt::Failed(error) => error,
        }
    }
}

impl EntityService {
    /// A run of one compare-and-set write, which ends once an attempt settles,
    /// fails, or the write budget is lost.
    ///
    /// A lost attempt is followed by the write backoff, which separates
    /// writers that collided rather than restarting them together.
    pub(crate) fn contended_run(&self) -> RetryRun<'_> {
        WRITE_RETRY.start(&*self.retry_delay)
    }
}
