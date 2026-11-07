//! The stable release a seal is made for, written as its tag `vX.Y.Z`.
//!
//! Only a stable release is sealed: a pre-release (`v1.0.0-rc.1`) ships nothing
//! a later release must keep.

use crate::release_tag::{ReleaseTag, ReleaseTagError};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::str::FromStr;

/// A stable release, ordered by its semantic version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display, SerializeDisplay, DeserializeFromStr)]
#[display("v{_0}")]
pub struct ReleaseVersion(semver::Version);

impl ReleaseVersion {
    /// The release `version`, which carries neither a pre-release nor build
    /// metadata.
    pub(crate) const fn new(version: semver::Version) -> ReleaseVersion {
        ReleaseVersion(version)
    }
}

impl FromStr for ReleaseVersion {
    type Err = ReleaseVersionError;

    /// Read the tag `vX.Y.Z` of a stable release.
    fn from_str(tag: &str) -> Result<ReleaseVersion, ReleaseVersionError> {
        match tag.parse::<ReleaseTag>()? {
            ReleaseTag::Stable(version) => Ok(version),
            ReleaseTag::PreRelease(version) => Err(ReleaseVersionError::PreRelease(version.pre)),
        }
    }
}

/// Why a tag names no stable release.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseVersionError {
    /// The text is no release tag.
    #[error(transparent)]
    Tag(#[from] ReleaseTagError),
    /// The tag names a pre-release.
    #[error("the tag names the pre-release {0}, and only a stable release vX.Y.Z is sealed")]
    PreRelease(semver::Prerelease),
}

#[cfg(test)]
mod tests {
    use crate::{
        release_tag::ReleaseTagError,
        release_version::{ReleaseVersion, ReleaseVersionError},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_stable_tag_reads_back_as_written_and_orders_by_version() -> Result<(), ReleaseVersionError> {
        let later = "v1.10.0".parse::<ReleaseVersion>()?;
        let earlier = "v1.2.3".parse::<ReleaseVersion>()?;
        assert_eq!((earlier.to_string(), later.to_string()), ("v1.2.3".to_owned(), "v1.10.0".to_owned()));
        assert!(earlier < later);
        Ok(())
    }

    #[test]
    fn a_pre_release_is_refused() {
        for tag in ["v1.0.0-alpha.1", "v1.0.0-beta.2", "v1.0.0-rc.3"] {
            assert!(
                matches!(tag.parse::<ReleaseVersion>(), Err(ReleaseVersionError::PreRelease(refused)) if tag.ends_with(refused.as_str())),
                "{tag}"
            );
        }
    }

    #[test]
    fn a_text_that_is_no_release_tag_is_refused() {
        assert!(matches!(
            "v1.0.0+build.7".parse::<ReleaseVersion>(),
            Err(ReleaseVersionError::Tag(ReleaseTagError::BuildMetadata(_)))
        ));
    }
}
