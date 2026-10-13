//! Durable consequences committed alongside the entity mutation.
//!
//! Each consequence is a distinct destination with its own row shape, so each
//! owns its own module; the committer sequences them inside one transaction and
//! knows nothing of their statements.

pub(crate) mod notification_events;
pub(crate) mod numbering;
pub(crate) mod side_effects;

/// Whether a commit's consequence phase persisted anything worth waking a drain for.
///
/// The committer uses this to decide whether to fire the matching edge
/// notification after the transaction commits: a phase that wrote nothing has no
/// drain to wake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConsequenceWrite {
    /// The phase persisted at least one row.
    Written,
    /// The phase had nothing to persist.
    Nothing,
}
