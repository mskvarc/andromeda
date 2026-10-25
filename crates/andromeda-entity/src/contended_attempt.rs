//! One attempt of a compare-and-set write, and the loop that repeats it while
//! a concurrent writer keeps superseding it.

use crate::{EntityService, error::EntityError};
use andromeda_context_commit::write_retry::{WRITE_ATTEMPTS, WRITE_RETRY};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_retry::retry_policy::RetryAttempt;
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
    /// Run `attempt` until it settles, fails, or loses the whole write budget.
    ///
    /// A lost attempt is followed by the write backoff, which separates
    /// writers that collided rather than restarting them together.
    ///
    /// # Errors
    ///
    /// The failure of the attempt that ended the loop.
    pub(crate) async fn retry_contended<T>(&self, attempt: impl AsyncFnMut(RetryAttempt) -> Result<T, ContendedAttempt>) -> Result<T, ContendedAttempt> {
        WRITE_RETRY.run(&*self.retry_delay, |failure, _| failure.is_lost(), attempt).await
    }
}
