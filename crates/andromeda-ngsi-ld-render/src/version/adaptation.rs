//! What a `Prefer: ngsi-ld=<version>` request did to a response payload.

use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

/// What a `Prefer: ngsi-ld=<version>` request did to a response payload.
///
/// Clause 6.3.6 makes the `Preference-Applied` echo unconditional once the
/// preference was expressed. It names the version the body conforms to,
/// while clause 4.3.6.8 makes the status depend on whether the body was
/// amended: a broker that amends a payload down to an older version is no
/// longer speaking authoritatively about it, so that response is 203 rather
/// than 200. Both decisions are taken after rendering by whoever writes the
/// response, so the outcome travels with the rendered output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAdaptation {
    /// The payload was amended to conform to `version`, an older NGSI-LD
    /// version than the one this broker conforms to.
    Applied {
        /// The version the amended payload conforms to.
        version: SpecVersion,
    },
    /// A version was requested but the payload already conforms to it: the
    /// requested version is the conformant one or newer, so nothing changed.
    Unchanged,
    /// No version preference was expressed.
    None,
}

impl VersionAdaptation {
    /// What a payload written for the requested `target` is: amended when the
    /// target predates the version this broker conforms to, unchanged
    /// otherwise.
    #[must_use]
    pub const fn requested(target: SpecVersion) -> VersionAdaptation {
        if target.precedes(SpecVersion::CONFORMANT) {
            VersionAdaptation::Applied { version: target }
        } else {
            VersionAdaptation::Unchanged
        }
    }

    /// The version to echo in `Preference-Applied` (clause 6.3.6), when a
    /// preference was expressed at all: the amended version, or the version
    /// this broker conforms to when the payload needed no amendment.
    #[must_use]
    pub const fn applied_version(self) -> Option<SpecVersion> {
        match self {
            VersionAdaptation::Applied { version } => Some(version),
            VersionAdaptation::Unchanged => Some(SpecVersion::CONFORMANT),
            VersionAdaptation::None => None,
        }
    }

    /// Whether the payload was amended, which is what turns a 200 into a 203.
    #[must_use]
    pub const fn is_amended(self) -> bool {
        matches!(self, VersionAdaptation::Applied { .. })
    }
}

#[cfg(test)]
mod tests {
    use crate::version::adaptation::VersionAdaptation;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

    /// Clause 4.3.6.8: a payload that needed no amendment is still
    /// authoritative, so the response stays 200 while clause 6.3.6 still echoes
    /// the version the body conforms to.
    #[test]
    fn an_unchanged_payload_echoes_the_conformant_version_and_is_not_amended() {
        let adaptation = VersionAdaptation::Unchanged;
        pretty_assertions::assert_eq!(adaptation.applied_version(), Some(SpecVersion::CONFORMANT));
        assert!(!adaptation.is_amended());
    }

    /// The echo names the version the amended body conforms to, not the version
    /// this broker conforms to.
    #[test]
    fn an_amended_payload_echoes_the_target_version_and_is_amended() {
        let adaptation = VersionAdaptation::Applied { version: SpecVersion::V1_4 };
        pretty_assertions::assert_eq!(adaptation.applied_version(), Some(SpecVersion::V1_4));
        assert!(adaptation.is_amended());
    }

    /// A requested version below the conformant one is amended to; the
    /// conformant version and every newer one leave the payload unchanged.
    #[test]
    fn a_requested_version_is_applied_only_below_the_conformant_version() {
        pretty_assertions::assert_eq!(
            VersionAdaptation::requested(SpecVersion::V1_0),
            VersionAdaptation::Applied { version: SpecVersion::V1_0 }
        );
        pretty_assertions::assert_eq!(VersionAdaptation::requested(SpecVersion::CONFORMANT), VersionAdaptation::Unchanged);
        pretty_assertions::assert_eq!(VersionAdaptation::requested(SpecVersion::new(2, 0)), VersionAdaptation::Unchanged);
    }

    /// Clause 6.3.6 ties the echo to the preference having been expressed, so a
    /// request that expressed none receives none.
    #[test]
    fn an_absent_preference_echoes_no_version() {
        pretty_assertions::assert_eq!(VersionAdaptation::None.applied_version(), None);
        assert!(!VersionAdaptation::None.is_amended());
    }
}
