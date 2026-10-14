//! What one temporal read knows of the branch it reads before its first
//! statement.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Whose history a read takes, and so which unrecorded history can change its
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeReach<'a> {
    /// The Temporal Evolution of every Entity of the branch.
    Every,
    /// The Temporal Evolution of one Entity alone: history held for any other
    /// Entity changes nothing the read answers.
    Entity(&'a Identifier),
}

/// Whether a branch held history its store had accepted and not yet recorded
/// when a read began.
///
/// A write acknowledged before the read began is either recorded or held by
/// then, so a read that found none held can read the recorded history alone
/// for the rest of its statements without missing anything a client could
/// see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnrecordedHistory {
    /// Some accepted history waited to be recorded.
    Held,
    /// Every accepted history was recorded.
    Absent,
}

/// The branch one temporal read reads, as its reader found it when the read
/// began; every statement of the read is answered over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalReadScope<'a> {
    /// The branch read.
    target: &'a ContextTarget,
    /// What the branch held unrecorded when the read began.
    unrecorded: UnrecordedHistory,
}

impl<'a> TemporalReadScope<'a> {
    /// A read of `target` that found `unrecorded`.
    #[must_use]
    pub const fn new(target: &'a ContextTarget, unrecorded: UnrecordedHistory) -> TemporalReadScope<'a> {
        TemporalReadScope { target, unrecorded }
    }

    /// The branch read.
    #[must_use]
    pub const fn target(&self) -> &'a ContextTarget {
        self.target
    }

    /// What the branch held unrecorded when the read began.
    #[must_use]
    pub const fn unrecorded(&self) -> UnrecordedHistory {
        self.unrecorded
    }
}

#[cfg(test)]
mod tests {
    use crate::port::read_scope::{TemporalReadScope, UnrecordedHistory};
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn a_scope_carries_its_branch_and_what_it_held_unrecorded() {
        let scope = TemporalReadScope::new(&ContextTarget::Live, UnrecordedHistory::Held);
        assert_eq!(scope.target(), &ContextTarget::Live);
        assert_eq!(scope.unrecorded(), UnrecordedHistory::Held);
    }
}
