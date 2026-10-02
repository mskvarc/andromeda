//! A write statement whose text depends on nothing but the branch it writes.
//!
//! The set-writes of the entity relations and of their attribute projection
//! are one statement per kind of change, whatever the batch: the rows travel
//! as bound arrays, so the statement text is a function of the branch schema
//! alone. The live branch -- every ordinary write -- builds its text once for
//! the process and reuses it; a Snapshot branch, written while a Snapshot is
//! captured or cloned and named by a schema of its own, builds its text for
//! each statement.

use crate::error::ContextStoreError;
use andromeda_context_space::context_target::ContextTarget;
use std::{borrow::Cow, sync::OnceLock};

/// How a statement's text is built for a branch.
type StatementBuilder = fn(&ContextTarget) -> Result<String, ContextStoreError>;

/// One statement, its live-branch text built on first use.
#[derive(Debug)]
pub(crate) struct BranchStatement {
    /// Builds the statement text for a branch.
    build: StatementBuilder,
    /// The live branch's text, once built.
    live: OnceLock<String>,
}

impl BranchStatement {
    /// The statement `build` writes.
    pub(crate) const fn new(build: StatementBuilder) -> BranchStatement {
        BranchStatement { build, live: OnceLock::new() }
    }

    /// The statement text for `target`.
    ///
    /// # Errors
    ///
    /// Returns the builder's [`ContextStoreError`] when the text cannot be
    /// built; a live-branch text that failed is built again on the next use.
    pub(crate) fn text(&self, target: &ContextTarget) -> Result<Cow<'_, str>, ContextStoreError> {
        match target {
            ContextTarget::Live => {
                if let Some(text) = self.live.get() {
                    return Ok(Cow::Borrowed(text));
                }
                let built = (self.build)(target)?;
                Ok(Cow::Borrowed(self.live.get_or_init(|| built)))
            }
            ContextTarget::Snapshot(_) => (self.build)(target).map(Cow::Owned),
        }
    }
}
