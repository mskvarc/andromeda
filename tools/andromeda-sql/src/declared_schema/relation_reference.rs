//! A relation a statement names without creating it, and a function it calls.

use crate::declared_schema::{origin::Origin, qualified_name::QualifiedName};

/// One relation a statement refers to: one it reads, writes, indexes, alters,
/// triggers on or references by a foreign key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationReference {
    /// The relation.
    name: QualifiedName,
    /// Where it is named.
    origin: Origin,
}

impl RelationReference {
    /// The reference to `name` at `origin`.
    #[must_use]
    pub const fn new(name: QualifiedName, origin: Origin) -> RelationReference {
        RelationReference { name, origin }
    }

    /// The relation.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// Where it is named.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}

/// Where a function is called from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPlace {
    /// An expression of a statement the formatter lays out.
    Statement,
    /// The `execute function` of a trigger.
    TriggerTarget,
    /// An expression of a `do` block, which the formatter lays out too.
    DoBlock,
}

/// One function call outside a function body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCall {
    /// The function.
    name: QualifiedName,
    /// Where it is called from.
    place: CallPlace,
    /// Where the call is written.
    origin: Origin,
}

impl FunctionCall {
    /// The call of `name` from `place` at `origin`.
    #[must_use]
    pub const fn new(name: QualifiedName, place: CallPlace, origin: Origin) -> FunctionCall {
        FunctionCall { name, place, origin }
    }

    /// The function.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// Where it is called from.
    #[must_use]
    pub const fn place(&self) -> CallPlace {
        self.place
    }

    /// Where the call is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
