//! Physical identifiers for role-neutral context-space partitions.

use sea_query::Iden;

/// The `context_spaces` parent relation.
#[derive(Clone, Copy, Iden)]
pub enum ContextSpaces {
    /// Table name: `context_spaces`.
    Table,
    /// Database-issued context-space key.
    ContextSpaceKey,
    /// Live or Snapshot partition discriminator.
    Kind,
    /// Creation instant for operational accounting.
    CreatedAt,
}

#[cfg(test)]
mod tests {
    use crate::context_space::ContextSpaces;
    use sea_query::Iden;

    #[test]
    fn context_space_relation_names_are_stable() {
        assert_eq!(ContextSpaces::Table.to_string(), "context_spaces");
        assert_eq!(ContextSpaces::ContextSpaceKey.to_string(), "context_space_key");
        assert_eq!(ContextSpaces::Kind.to_string(), "kind");
    }
}
