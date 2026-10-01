//! Whether a store failure can be retried unchanged, and why.

use derive_more::IsVariant;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum RetryCause {
    /// The store is under load: no connection became free in time.
    Saturated,
    /// The store rolled the transaction back under contention.
    Contended,
    /// The store could not be reached, or the connection to it was lost.
    Unreachable,
}
