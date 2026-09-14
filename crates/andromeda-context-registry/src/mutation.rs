//! Outcomes of Context Registry persistence mutations.

/// Whether a persistence mutation found its target Context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMutation {
    /// A stored Context was changed or removed.
    Applied,
    /// No matching stored Context existed.
    Missing,
}

#[cfg(test)]
mod tests {
    use crate::mutation::ContextMutation;
    use pretty_assertions::assert_ne;

    #[test]
    fn mutation_outcomes_are_not_interchangeable() {
        assert_ne!(ContextMutation::Applied, ContextMutation::Missing);
    }
}
