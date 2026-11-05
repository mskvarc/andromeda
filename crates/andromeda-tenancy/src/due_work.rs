//! Whether the work a runtime was loaded for has run.
//!
//! A tenant loaded for due work keeps its place for its woken lifetime, so its
//! work runs before anything unloads it. Most of that work takes far less than
//! the lifetime, and while every place is taken the rest of the lifetime holds
//! the place for nothing. The cache asks such a runtime whether its work has
//! run, and a runtime that answers it has gives up its place to the next due
//! tenant. Only the runtime can answer: what the work is, and where it is
//! read, is the composition's.

use async_trait::async_trait;

/// Whether the work a runtime owes has run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DueWork {
    /// Something the tenant owes is due now, the runtime holds a change it has
    /// not acted on, or the answer could not be read.
    Outstanding,
    /// Nothing the tenant owes is due now, and the runtime holds no change it
    /// has not acted on.
    Drained,
}

/// Reads whether one loaded runtime's due work has run.
#[async_trait]
pub trait DueWorkSource: Send + Sync + 'static {
    /// Whether the runtime's due work has run, read now.
    ///
    /// A read that fails answers [`DueWork::Outstanding`]: a runtime is never
    /// unloaded early on an answer nobody could read.
    #[must_use = "a due-work read must be awaited"]
    async fn due_work(&self) -> DueWork;
}
