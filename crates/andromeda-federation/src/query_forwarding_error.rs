//! Why a forwarded entity query produced no answer.

use crate::{entity_budget::EntityBudgetExceeded, error::FederationError};
use thiserror::Error;

/// A forwarded Query Entities (clause 5.7.2.4) that could not be answered.
///
/// Beside every failure a forwarding can end in, a query walks each source's
/// answer to its end, which can gather more entities than the broker answers
/// one request with; that refusal is the operation's (clause 4.12,
/// `TooManyResults`), never a warning, because a partial collection would make
/// the page window and `NGSILD-Results-Count` silently wrong.
#[derive(Debug, Error)]
pub enum QueryForwardingError {
    /// The forwarding itself failed.
    #[error(transparent)]
    Federation(#[from] FederationError),
    /// The sources' answers exceed what one request gathers.
    #[error(transparent)]
    TooManyResults(#[from] EntityBudgetExceeded),
}
