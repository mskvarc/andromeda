//! Which URLs the broker accepts as the NGSI-LD core context (clause 4.4), and
//! the document it resolves each to.
//!
//! Every released core context is recognised, and each URL resolves from
//! memory to the document its publisher serves under it, so no core context is
//! ever fetched.

use andromeda_ngsi_ld_vocabulary::{
    NGSI_LD_CORE_CONTEXT_URL,
    core_context::{
        NGSI_LD_CORE_CONTEXT_V10,
        NGSI_LD_CORE_CONTEXT_V10_URL,
        NGSI_LD_CORE_CONTEXT_V13,
        NGSI_LD_CORE_CONTEXT_V13_URL,
        NGSI_LD_CORE_CONTEXT_V14,
        NGSI_LD_CORE_CONTEXT_V14_URL,
        NGSI_LD_CORE_CONTEXT_V15,
        NGSI_LD_CORE_CONTEXT_V15_URL,
        NGSI_LD_CORE_CONTEXT_V16,
        NGSI_LD_CORE_CONTEXT_V16_URL,
        NGSI_LD_CORE_CONTEXT_V17,
        NGSI_LD_CORE_CONTEXT_V17_URL,
        NGSI_LD_CORE_CONTEXT_V18,
        NGSI_LD_CORE_CONTEXT_V18_URL,
        NGSI_LD_CORE_CONTEXT_V19,
    },
};
use iri_rs::Iri;

/// The URL of every released NGSI-LD core context, oldest first.
pub const NGSI_LD_CORE_CONTEXT_URL_LIST: [Iri<&'static str>; 8] = [
    NGSI_LD_CORE_CONTEXT_V10_URL,
    NGSI_LD_CORE_CONTEXT_V13_URL,
    NGSI_LD_CORE_CONTEXT_V14_URL,
    NGSI_LD_CORE_CONTEXT_V15_URL,
    NGSI_LD_CORE_CONTEXT_V16_URL,
    NGSI_LD_CORE_CONTEXT_V17_URL,
    NGSI_LD_CORE_CONTEXT_V18_URL,
    NGSI_LD_CORE_CONTEXT_URL,
];

/// Lookup table from core context URL to its embedded JSON payload.
///
/// Every URL maps to the document its publisher serves under it.
pub const NGSI_LD_CORE_CONTEXT_MAP: [(Iri<&'static str>, &str); 8] = [
    (NGSI_LD_CORE_CONTEXT_V10_URL, NGSI_LD_CORE_CONTEXT_V10),
    (NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V13),
    (NGSI_LD_CORE_CONTEXT_V14_URL, NGSI_LD_CORE_CONTEXT_V14),
    (NGSI_LD_CORE_CONTEXT_V15_URL, NGSI_LD_CORE_CONTEXT_V15),
    (NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V16),
    (NGSI_LD_CORE_CONTEXT_V17_URL, NGSI_LD_CORE_CONTEXT_V17),
    (NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_V18),
    (NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19),
];

/// Returns `true` if `url` matches any known NGSI-LD core context version.
#[must_use]
pub fn is_ngsi_ld_core_context_url(url: Iri<&str>) -> bool {
    NGSI_LD_CORE_CONTEXT_URL_LIST.iter().any(|candidate| candidate == &url)
}

/// The embedded core-context document associated with a known URL.
#[must_use]
pub fn embedded_core_context(url: Iri<&str>) -> Option<&'static str> {
    NGSI_LD_CORE_CONTEXT_MAP
        .iter()
        .find_map(|(candidate, payload)| (candidate == &url).then_some(*payload))
}

#[cfg(test)]
mod tests {
    use crate::core_context::{NGSI_LD_CORE_CONTEXT_MAP, embedded_core_context, is_ngsi_ld_core_context_url};
    use andromeda_ngsi_ld_vocabulary::{
        NGSI_LD_CORE_CONTEXT_URL,
        core_context::{NGSI_LD_CORE_CONTEXT_V10, NGSI_LD_CORE_CONTEXT_V10_URL, NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V19},
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
        for (url, payload) in NGSI_LD_CORE_CONTEXT_MAP {
            let sharing = NGSI_LD_CORE_CONTEXT_MAP
                .iter()
                .filter(|(_other, other_payload)| *other_payload == payload)
                .count();
            pretty_assertions::assert_eq!(sharing, 1, "{url} shares its payload with another URL");
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
