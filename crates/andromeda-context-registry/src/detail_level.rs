//! Projection detail requested for Context reads.

/// Whether a Context operation returns its document or full lifecycle metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextDetailLevel {
    /// Return identifiers from lists and the JSON-LD document from a single read.
    Summary,
    /// Return full metadata records.
    Full,
}

#[cfg(test)]
mod tests {
    use crate::detail_level::ContextDetailLevel;
    use pretty_assertions::assert_ne;

    #[test]
    fn summary_and_full_are_distinct_projections() {
        assert_ne!(ContextDetailLevel::Summary, ContextDetailLevel::Full);
    }
}
