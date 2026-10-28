//! The tag a release is pushed under: `v` and a semantic version, a stable
//! release `vX.Y.Z` or a pre-release such as `v1.0.0-rc1`.
//!
//! Build metadata names no release of its own, so a tag never carries it.

use crate::release_version::ReleaseVersion;
use std::str::FromStr;

/// The prefix of every release tag.
const TAG_PREFIX: &str = "v";

/// A release tag.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub enum ReleaseTag {
    /// A stable release, sealed in the ledger before it is tagged.
    #[display("{_0}")]
    Stable(ReleaseVersion),
    /// A pre-release, which ships nothing a later release must keep and is
    /// never sealed.
    #[display("v{_0}")]
    PreRelease(semver::Version),
}

impl FromStr for ReleaseTag {
    type Err = ReleaseTagError;

    /// Read the tag `vX.Y.Z` or `vX.Y.Z-<pre-release>`.
    fn from_str(tag: &str) -> Result<ReleaseTag, ReleaseTagError> {
        let version = tag.strip_prefix(TAG_PREFIX).ok_or(ReleaseTagError::NoPrefix)?;
        let version = semver::Version::parse(version).map_err(ReleaseTagError::NotSemantic)?;
        if !version.build.is_empty() {
            return Err(ReleaseTagError::BuildMetadata(version.build));
        }
        Ok(if version.pre.is_empty() {
            ReleaseTag::Stable(ReleaseVersion::new(version))
        } else {
            ReleaseTag::PreRelease(version)
        })
    }
}

/// Why a text is no release tag.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseTagError {
    /// The tag does not start with `v`.
    #[error("a release tag is `v` followed by a semantic version, vX.Y.Z")]
    NoPrefix,
    /// What follows the `v` is not a semantic version.
    #[error("a release tag is `v` followed by a semantic version, vX.Y.Z: {0}")]
    NotSemantic(semver::Error),
    /// The tag carries build metadata.
    #[error("the tag carries the build metadata {0}, and a release is tagged as plain vX.Y.Z")]
    BuildMetadata(semver::BuildMetadata),
}

#[cfg(test)]
mod tests {
    use crate::release_tag::{ReleaseTag, ReleaseTagError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_stable_tag_and_a_pre_release_tag_read_back_as_written() -> Result<(), ReleaseTagError> {
        let stable = "v1.2.3".parse::<ReleaseTag>()?;
        let candidate = "v1.2.3-rc1".parse::<ReleaseTag>()?;
        assert!(matches!(stable, ReleaseTag::Stable(_)));
        assert!(matches!(candidate, ReleaseTag::PreRelease(_)));
        assert_eq!((stable.to_string(), candidate.to_string()), ("v1.2.3".to_owned(), "v1.2.3-rc1".to_owned()));
        Ok(())
    }

    #[test]
    fn a_tag_that_is_not_v_and_a_plain_semantic_version_is_refused() {
        assert!(matches!("1.0.0".parse::<ReleaseTag>(), Err(ReleaseTagError::NoPrefix)));
        assert!(matches!("v1.0".parse::<ReleaseTag>(), Err(ReleaseTagError::NotSemantic(_))));
        assert!(matches!("v1.0.0+build.7".parse::<ReleaseTag>(), Err(ReleaseTagError::BuildMetadata(_))));
    }
}
