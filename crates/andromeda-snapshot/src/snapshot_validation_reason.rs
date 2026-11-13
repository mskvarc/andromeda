//! The concrete reasons a Snapshot request is rejected as `BadRequestData`.

use andromeda_ngsi_ld_model::{notification_header_field::NotificationHeaderFieldError, snapshot::SnapshotStatus};
use andromeda_ngsi_ld_query::error::QueryError;
use thiserror::Error;

/// Why a Snapshot request violates Snapshot semantics (clause 5.16).
#[derive(Debug, Error)]
pub enum SnapshotValidationReason {
    /// Neither query collection was given.
    #[error("at least one of snapshotQueries or snapshotTemporalQueries must be present")]
    QueryCollectionsMissing,
    /// A member of `snapshotQueries` carries a `temporalQ`.
    #[error("invalid snapshotQueries[{index}]: temporalQ is not applicable")]
    TemporalQOnEntityQuery {
        /// Position of the offending query.
        index: usize,
    },
    /// A member of `snapshotTemporalQueries` omits its `temporalQ`.
    #[error("invalid snapshotTemporalQueries[{index}]: missing temporalQ")]
    TemporalQueryWithoutTemporalQ {
        /// Position of the offending query.
        index: usize,
    },
    /// A member of `snapshotTemporalQueries` carries a `temporalQ` whose
    /// `timerel`, `timeAt` and `endTimeAt` name no interval.
    #[error("invalid snapshotTemporalQueries[{index}]: {source}")]
    TemporalQWindow {
        /// Position of the offending query.
        index: usize,
        /// The window fault.
        source: QueryError,
    },
    /// A patch named a Snapshot id other than the one it patches.
    #[error("the Snapshot id is immutable")]
    IdImmutable,
    /// A patch touched the query definitions.
    #[error("the Snapshot query definitions are immutable")]
    QueryDefinitionsImmutable,
    /// The requested lifetime carries the expiry outside the representable range.
    #[error("snapshotLifetime is outside the representable time range")]
    LifetimeOutOfRange,
    /// The requested lifetime is not positive.
    #[error("snapshotLifetime must be positive")]
    LifetimeNotPositive,
    /// The status state machine forbids leaving `preparing` for this status.
    #[error("invalid status transition: preparing -> {status}")]
    StatusTransition {
        /// The status the transition asked for.
        status: SnapshotStatus,
    },
    /// An `http` or `https` endpoint's `receiverInfo` holds an entry that is not
    /// an HTTP header field (clause 5.16.6, Table 5.2.22-1), or one that names a
    /// field the broker writes on every notification itself.
    #[error("invalid receiverInfo: {0}")]
    ReceiverInfo(#[source] NotificationHeaderFieldError),
}
