//! The conformance ratchet's authoritative list of known gaps.
//!
//! Every `#[ignore = "GAP REQ-…"]` test in the suite corresponds to one entry
//! here. The list is the machine-readable backlog the CI report reads: when an
//! ignored gap test starts passing, the report fails until the requirement is
//! promoted and its entry removed. Keeping the reasons in one place also keeps
//! the `#[ignore]` strings consistent with the register.

/// One tracked gap: the requirement it blocks and why it is not yet testable green.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapEntry {
    /// The `REQ-<AREA>-<n>` identifier from the conformance register.
    pub requirement: &'static str,
    /// One-line reason, mirrored in the test's `#[ignore]` string.
    pub reason: &'static str,
}

/// The complete set of requirements encoded as tracked expected-failures.
///
/// The CSR health counters, the caching and cooldown warning codes and the
/// durable cross-instance entity maps that once lived here are implemented;
/// what remains is the source-identity 501 branch, which this broker never
/// takes because its identity is always available.
pub const KNOWN_GAPS: &[GapEntry] = &[GapEntry {
    requirement: "REQ-MAP-160",
    reason: "source-identity 501 unsupported branch not implemented",
}];

#[cfg(test)]
mod tests {
    use crate::gaps::KNOWN_GAPS;
    use std::collections::BTreeSet;

    #[test]
    fn known_gaps_have_unique_requirement_ids() {
        let mut seen = BTreeSet::new();
        for gap in KNOWN_GAPS {
            assert!(seen.insert(gap.requirement), "duplicate gap entry for {}", gap.requirement);
        }
    }

    #[test]
    fn known_gaps_carry_nonempty_reasons() {
        for gap in KNOWN_GAPS {
            assert!(!gap.reason.is_empty(), "gap {} has no reason", gap.requirement);
        }
    }
}
