//! How strongly a transaction locks the identity rows it writes.

use sea_query::LockType;

/// The row lock a transaction takes on every identity row it writes before
/// any of its writes run: a commit of Entity writes, and the application of
/// queued history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityRowLock {
    /// `FOR NO KEY UPDATE`, the lock an update of a non-key column takes: it
    /// waits for and blocks every write of the row, a Temporal Evolution
    /// deletion's `FOR UPDATE` included, and lets a foreign key check into the
    /// row pass.
    Writing,
    /// `FOR UPDATE`, the lock a deletion of the row takes, held by a commit
    /// that deletes any of its rows, so its deletion needs no lock stronger
    /// than the one it holds.
    Deleting,
}

impl IdentityRowLock {
    /// The lock clause the statement takes.
    #[must_use]
    pub(crate) const fn lock_type(self) -> LockType {
        match self {
            IdentityRowLock::Writing => LockType::NoKeyUpdate,
            IdentityRowLock::Deleting => LockType::Update,
        }
    }
}
