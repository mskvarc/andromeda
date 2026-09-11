//! How one distributed operation through a registration ended (ETSI GS CIM 009 Table 5.2.9-2).

/// How one distributed operation through a registration ended, for the
/// read-only health members of Table 5.2.9-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingOutcome {
    /// The source answered the operation.
    Succeeded,
    /// The source failed to answer, or answered with an error.
    Failed,
}
