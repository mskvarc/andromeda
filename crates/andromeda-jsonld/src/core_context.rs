//! Which URLs the broker accepts as the NGSI-LD core context (clause 4.4), and
//! the document it resolves each to.
//!
//! Every released core context is recognised, and each URL resolves from
//! memory to the document its publisher serves under it, so no core context is
//! ever fetched.

use andromeda_ngsi_ld_vocabulary::core_context::NGSI_LD_CORE_CONTEXT_RELEASES;
use iri_rs::Iri;

/// Returns `true` if `url` matches any known NGSI-LD core context version.
#[must_use]
pub fn is_ngsi_ld_core_context_url(url: Iri<&str>) -> bool {
    NGSI_LD_CORE_CONTEXT_RELEASES.iter().any(|release| release.url() == url)
}

/// The embedded core-context document associated with a known URL.
#[must_use]
pub fn embedded_core_context(url: Iri<&str>) -> Option<&'static str> {
    NGSI_LD_CORE_CONTEXT_RELEASES
        .iter()
        .find_map(|release| (release.url() == url).then_some(release.document()))
}

#[cfg(test)]
mod tests {
    use crate::core_context::{embedded_core_context, is_ngsi_ld_core_context_url};
    use andromeda_ngsi_ld_vocabulary::{
        NGSI_LD_CORE_CONTEXT_URL,
        core_context::{
            NGSI_LD_CORE_CONTEXT_RELEASES,
            NGSI_LD_CORE_CONTEXT_V10,
            NGSI_LD_CORE_CONTEXT_V10_URL,
            NGSI_LD_CORE_CONTEXT_V16_URL,
            NGSI_LD_CORE_CONTEXT_V19,
        },
    };
    use iri_rs::iri;

    #[test]
    fn recognises_latest_core_context() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_URL.as_ref()));
    }

    #[test]
    fn recognises_the_versionless_version_1_0_url() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_V10_URL.as_ref()));
    }

    #[test]
    fn the_versionless_url_resolves_to_the_version_1_0_document() {
        pretty_assertions::assert_eq!(embedded_core_context(NGSI_LD_CORE_CONTEXT_V10_URL), Some(NGSI_LD_CORE_CONTEXT_V10));
    }

    #[test]
    fn the_v1_9_url_resolves_to_the_v1_9_document() {
        pretty_assertions::assert_eq!(embedded_core_context(NGSI_LD_CORE_CONTEXT_URL), Some(NGSI_LD_CORE_CONTEXT_V19));
    }

    #[test]
    fn every_core_context_url_resolves_to_its_own_release() {
        for release in NGSI_LD_CORE_CONTEXT_RELEASES {
            pretty_assertions::assert_eq!(embedded_core_context(release.url()), Some(release.document()), "{}", release.url());
            let sharing = NGSI_LD_CORE_CONTEXT_RELEASES
                .iter()
                .filter(|other| other.document() == release.document())
                .count();
            pretty_assertions::assert_eq!(sharing, 1, "{} shares its document with another URL", release.url());
        }
    }

    #[test]
    fn recognises_v16() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_V16_URL.as_ref()));
    }

    #[test]
    fn rejects_unknown_url() {
        assert!(!is_ngsi_ld_core_context_url(iri!("https://example.com/context.jsonld")));
    }
}
