//! How a payload written for an older specification version deletes a member.
//!
//! Clause 4.3.6.6 amends what a broker forwards to the version a source
//! conforms to, and the encoding of a deletion is one of the things the
//! versions disagree on. Clause 5.5.8 of V1.4.2 removes "each member of the
//! Fragment, whose value is null", the JSON `null` of IETF RFC 7396, but its
//! clause 5.6.4.4 adds that "the value of a Property or the object of a
//! Relationship shall not be set to null in order to delete it". V1.5.1 drops
//! the rule and refuses any null member value (clause 5.5.4), so it deletes
//! nothing. From V1.6.1 a deletion is the NGSI-LD Null of clause 4.5.0. Where a
//! version has no encoding, the deletion is left out of the payload, as Tables
//! 4.3.6.8-1 to -3 leave out every member a target cannot read.

use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

/// Where a deletion stands, which decides whether a version before 1.6 can
/// write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionSite {
    /// A member of a fragment the update merges by clause 5.5.8: a member of
    /// a Subscription, or a member or sub-attribute of the attribute a Partial
    /// Attribute Update names (clause 5.6.4).
    MergedMember,
    /// An attribute, or the value or object of one, whose deletion deletes the
    /// attribute: the value of the attribute a Partial Attribute Update names,
    /// and every member of an entity body, which no version before 1.6 merges.
    Attribute,
}

/// How a payload writes a deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionEncoding {
    /// The NGSI-LD Null of clause 4.5.0.
    NgsiLdNull,
    /// The JSON `null` a clause 5.5.8 merge reads up to V1.4.
    JsonNull,
}

impl DeletionEncoding {
    /// How a payload written for `target` writes a deletion at `site`, or
    /// `None` when the target cannot read one there and the deletion is left
    /// out. No target is a source of the version this broker conforms to.
    #[must_use]
    pub const fn for_target(target: Option<SpecVersion>, site: DeletionSite) -> Option<DeletionEncoding> {
        let Some(target) = target else {
            return Some(DeletionEncoding::NgsiLdNull);
        };
        if !target.precedes(SpecVersion::V1_6) {
            Some(DeletionEncoding::NgsiLdNull)
        } else if !target.precedes(SpecVersion::V1_5) {
            None
        } else {
            match site {
                DeletionSite::MergedMember => Some(DeletionEncoding::JsonNull),
                DeletionSite::Attribute => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::version::deletion::{DeletionEncoding, DeletionSite};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use pretty_assertions::assert_eq;

    const SITES: [DeletionSite; 2] = [DeletionSite::MergedMember, DeletionSite::Attribute];

    #[test]
    fn a_source_of_the_conformant_version_reads_the_ngsi_ld_null() {
        for site in SITES {
            assert_eq!(DeletionEncoding::for_target(None, site), Some(DeletionEncoding::NgsiLdNull));
        }
    }

    #[test]
    fn a_source_from_1_6_reads_the_ngsi_ld_null_everywhere() {
        for target in [
            SpecVersion::V1_6,
            SpecVersion::new(1, 7),
            SpecVersion::V1_8,
            SpecVersion::V1_9,
            SpecVersion::new(2, 0),
        ] {
            for site in SITES {
                assert_eq!(
                    DeletionEncoding::for_target(Some(target), site),
                    Some(DeletionEncoding::NgsiLdNull),
                    "{target} {site:?}"
                );
            }
        }
    }

    /// V1.5.1 clause 5.5.4: "Any attempt to use null as member value shall
    /// result in an error of type `BadRequestData`", and its clause 5.5.8 deletes
    /// nothing.
    #[test]
    fn a_1_5_source_reads_no_deletion() {
        for site in SITES {
            assert_eq!(DeletionEncoding::for_target(Some(SpecVersion::V1_5), site), None);
        }
    }

    /// V1.4.2 clauses 5.5.8 and 5.6.4.4: a merged member is removed by JSON
    /// `null`, the value or object of an attribute never is.
    #[test]
    fn a_source_up_to_1_4_reads_json_null_for_a_merged_member_only() {
        for target in [SpecVersion::V1_0, SpecVersion::V1_3, SpecVersion::V1_4] {
            assert_eq!(
                DeletionEncoding::for_target(Some(target), DeletionSite::MergedMember),
                Some(DeletionEncoding::JsonNull),
                "{target}"
            );
            assert_eq!(DeletionEncoding::for_target(Some(target), DeletionSite::Attribute), None, "{target}");
        }
    }
}
