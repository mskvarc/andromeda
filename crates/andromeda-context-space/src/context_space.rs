//! Role-neutral context-space records.

use crate::context_space_key::ContextSpaceKey;

/// Persistent role of one tenant context space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextSpaceKind {
    /// The tenant's unique ordinary context branch.
    Live,
    /// A branch owned by one Snapshot resource.
    Snapshot,
}

/// Durable context branch shared by entity, temporal, and subscription state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextSpace {
    key: ContextSpaceKey,
    kind: ContextSpaceKind,
}

impl ContextSpace {
    /// Construct a context-space record returned by persistence.
    #[must_use]
    pub const fn new(key: ContextSpaceKey, kind: ContextSpaceKind) -> ContextSpace {
        ContextSpace { key, kind }
    }

    /// Return the durable branch identity.
    #[must_use]
    pub const fn key(self) -> ContextSpaceKey {
        self.key
    }

    /// Return the branch role.
    #[must_use]
    pub const fn kind(self) -> ContextSpaceKind {
        self.kind
    }
}
