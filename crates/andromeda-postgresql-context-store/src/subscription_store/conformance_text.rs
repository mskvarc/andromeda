//! The text a subscription's `ngsildConformance` is stored as.
//!
//! Table 5.2.12-1 defines the member as text of the form `major.minor`, which
//! is what a retrieve has to echo, so the column holds the rendering of the
//! parsed version rather than a decomposed encoding no query compares.

use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

/// The stored `major.minor` text of `version`, or `None` when the
/// subscription declares no version.
pub(crate) fn stored_conformance(version: Option<SpecVersion>) -> Option<String> {
    version.map(|version| version.to_string())
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::conformance_text::stored_conformance;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_declared_version_is_stored_as_the_text_it_renders_as() {
        let version: SpecVersion = "1.8".parse().expect("a valid version");
        assert_eq!(stored_conformance(Some(version)), Some("1.8".to_owned()));
    }

    #[test]
    fn no_declared_version_stores_nothing() {
        assert_eq!(stored_conformance(None), None);
    }
}
