//! Context deletion and reload lifecycle commands.

/// The operation selected for a Context DELETE request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextRemoval {
    /// Delete the registered Context.
    Delete,
    /// Re-download and replace a cached Context.
    Reload,
}

#[cfg(test)]
mod tests {
    use crate::removal::ContextRemoval;
    use pretty_assertions::assert_ne;

    #[test]
    fn delete_and_reload_are_distinct_commands() {
        assert_ne!(ContextRemoval::Delete, ContextRemoval::Reload);
    }
}
