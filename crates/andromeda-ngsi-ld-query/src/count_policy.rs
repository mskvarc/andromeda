//! Whether a query reports the total number of its matches (clause 6.3.13).

/// Whether a query reports the total number of matches (clause 6.3.13).
///
/// The distinction is not cosmetic: emitting the count means running a second
/// statement over the same predicate, so this decides whether that statement is
/// built at all.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountPolicy {
    /// Build and run the count statement, and emit `NGSILD-Results-Count`.
    Emit,
    /// Skip the count statement entirely.
    #[default]
    Omit,
}
