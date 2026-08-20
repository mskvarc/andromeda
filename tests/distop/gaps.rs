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
/// Sourced from the implementation recon in the test plan (§6.3): CSR health
/// counter transitions, CSR caching/cooldown warning codes, cross-instance and
/// durable entity-map behaviour, and the unsupported source-identity branch.
pub const KNOWN_GAPS: &[GapEntry] = &[
    GapEntry {
        requirement: "REQ-REG-31",
        reason: "dynamic CSR status transition to failed on forward error not implemented",
    },
    GapEntry {
        requirement: "REQ-REG-32",
        reason: "dynamic CSR health counter increment on forward attempt not implemented",
    },
    GapEntry {
        requirement: "REQ-HTTP-16",
        reason: "CSR cache staleness 110 warning not implemented",
    },
    GapEntry {
        requirement: "REQ-HTTP-17",
        reason: "CSR cache staleness 110 warning not implemented",
    },
    GapEntry {
        requirement: "REQ-HTTP-19",
        reason: "CSR cache staleness 110 warning not implemented",
    },
    GapEntry {
        requirement: "REQ-HTTP-31",
        reason: "CSR cooldown decline not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-8",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-13",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-26",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-45",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-48",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-CONS-61",
        reason: "cross-instance linked entity-map forwarding not implemented",
    },
    GapEntry {
        requirement: "REQ-DISC-112",
        reason: "cross-instance loop prevention over durable map not implemented",
    },
    GapEntry {
        requirement: "REQ-DISC-113",
        reason: "cross-instance loop prevention over durable map not implemented",
    },
    GapEntry {
        requirement: "REQ-FW-49",
        reason: "durable / cross-instance entity-map sharing not implemented",
    },
    GapEntry {
        requirement: "REQ-FW-51",
        reason: "durable / cross-instance entity-map sharing not implemented",
    },
    GapEntry {
        requirement: "REQ-MAP-160",
        reason: "source-identity 501 unsupported branch not implemented",
    },
];

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
