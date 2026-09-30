//! Whether a new Context was stored under the identifier it was offered.

/// The outcome of storing a new Context under a chosen identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextInsertion {
    /// The Context is now stored under the identifier.
    Inserted,
    /// Another Context already holds the identifier, and nothing was stored.
    Occupied,
}

#[cfg(test)]
mod tests {
    use crate::insertion::ContextInsertion;
    use pretty_assertions::assert_ne;

    #[test]
    fn insertion_outcomes_are_not_interchangeable() {
        assert_ne!(ContextInsertion::Inserted, ContextInsertion::Occupied);
    }
}
