//! Whether a store failure can be retried unchanged, and why.

use derive_more::{Display, IsVariant};

/// Whether an operation that failed in the store can be sent again unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum Retryability {
    /// Nothing of the operation is known to have persisted; the identical
    /// operation can be sent again for the given cause.
    Retryable(RetryCause),
    /// A fault no retry mends.
    Fault,
}

/// Why a failed operation can be sent again unchanged.
///
/// The rendering is the detail a client is told, the one text every port's
/// retryable category reports for the cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IsVariant)]
pub enum RetryCause {
    /// The store is under load: no connection became free in time.
    #[display("the datastore connection pool is saturated; retry shortly")]
    Saturated,
    /// The store rolled the transaction back under contention.
    #[display("the operation was rolled back by a concurrent transaction; retry shortly")]
    Contended,
    /// The store could not be reached, or the connection to it was lost.
    #[display("the datastore is temporarily unavailable; retry shortly")]
    Unreachable,
}

#[cfg(test)]
mod tests {
    use crate::retryability::RetryCause;

    #[test]
    fn each_cause_renders_the_detail_a_client_is_told() {
        pretty_assertions::assert_eq!(RetryCause::Saturated.to_string(), "the datastore connection pool is saturated; retry shortly");
        pretty_assertions::assert_eq!(
            RetryCause::Contended.to_string(),
            "the operation was rolled back by a concurrent transaction; retry shortly"
        );
        pretty_assertions::assert_eq!(RetryCause::Unreachable.to_string(), "the datastore is temporarily unavailable; retry shortly");
    }
}
