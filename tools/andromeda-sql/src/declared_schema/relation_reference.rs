//! A relation a statement names without creating it.

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
