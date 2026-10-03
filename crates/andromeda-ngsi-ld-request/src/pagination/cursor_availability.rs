//! Whether the broker hands out and follows opaque cursors at all.

/// Whether cursor pagination (clause 5.5.9.1) is available, and if not, who
/// turned it off.
///
/// Who turned it off matters to the policy: an operator who disabled cursors
/// and also made them the default has written a contradiction, while the
/// conformance override forcing them off is an override of whatever the
/// operator wrote, and the default falls back to offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorAvailability {
    /// Cursors are handed out and followed.
    Enabled,
    /// The operator disabled cursor pagination.
    DisabledByOperator,
    /// The conformance override forced cursor pagination off.
    ForcedOffByConformance,
}
