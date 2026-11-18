//! Why the `timerel`, `timeAt` and `endTimeAt` of a request bound no interval
//! (clause 4.11).

use thiserror::Error;

/// The clause 4.11 rule the temporal parameters of a request broke.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TimeIntervalError {
    /// `timerel` and `timeAt` must both be present or both absent.
    #[error("timerel and timeAt must both be present or both absent")]
    UnpairedTimerel,
    /// `timerel=between` was provided without an `endTimeAt`.
    #[error("endTimeAt is required when timerel is 'between'")]
    EndTimeAtRequired,
    /// An `endTimeAt` came with a `timerel` other than `between`, or with none.
    #[error("endTimeAt is only allowed when timerel is 'between'")]
    EndTimeAtWithoutBetween,
    /// A `between` window's `endTimeAt` is at or before its `timeAt`.
    #[error("endTimeAt must be later than timeAt")]
    EndTimeAtNotAfterTimeAt,
}
