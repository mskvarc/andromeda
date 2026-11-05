//! How strongly a commit locks the identity rows it writes.

use sea_query::LockType;

/// The row lock a commit takes on every identity row it writes before any of
/// its writes run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitRowLock {
    /// `FOR NO KEY UPDATE`, the lock an update of a non-key column takes: it
    /// waits for and blocks every write of the row, and lets a foreign key
    /// check into the row pass.
    Writing,
    /// `FOR UPDATE`, the lock a deletion of the row takes, held by a commit
    /// that deletes any of its rows, so its deletion needs no lock stronger
    /// than the one it holds.
    Deleting,
}

impl CommitRowLock {
    /// The lock clause the statement takes.
    #[must_use]
    pub(crate) const fn lock_type(self) -> LockType {
        match self {
            CommitRowLock::Writing => LockType::NoKeyUpdate,
            CommitRowLock::Deleting => LockType::Update,
        }
    }
}
