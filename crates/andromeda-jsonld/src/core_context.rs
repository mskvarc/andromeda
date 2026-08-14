//! NGSI-LD core context URL constants and embedded payloads.
//!
//! Provides canonical URLs for all released versions of the NGSI-LD core
//! context (ETSI CIM 009 / GS CIM 009) and helpers to test whether a URL
//! matches any known version.
//!
//! The embedded payloads are compiled into the binary via [`include_str!`] so
//! they are available without any network access.

use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::{Iri, iri};
use std::ptr;

/// Core Context v1.3 URL.
pub const NGSI_LD_CORE_CONTEXT_V13_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld");
/// Core Context v1.3 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V13: &str = include_str!("context/ngsi-ld-core-context-v1.3.jsonld");

/// Core Context v1.4 URL.
pub const NGSI_LD_CORE_CONTEXT_V14_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld");
/// Core Context v1.4 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V14: &str = include_str!("context/ngsi-ld-core-context-v1.4.jsonld");

/// Core Context v1.5 URL.
pub const NGSI_LD_CORE_CONTEXT_V15_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld");
/// Core Context v1.5 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V15: &str = include_str!("context/ngsi-ld-core-context-v1.5.jsonld");

/// Core Context v1.6 URL.
pub const NGSI_LD_CORE_CONTEXT_V16_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld");
/// Core Context v1.6 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V16: &str = include_str!("context/ngsi-ld-core-context-v1.6.jsonld");

/// Core Context v1.7 URL.
pub const NGSI_LD_CORE_CONTEXT_V17_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld");
/// Core Context v1.7 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V17: &str = include_str!("context/ngsi-ld-core-context-v1.7.jsonld");

/// Core Context v1.8 URL.
pub const NGSI_LD_CORE_CONTEXT_V18_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");
/// Core Context v1.8 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V18: &str = include_str!("context/ngsi-ld-core-context-v1.8.jsonld");

/// NGSI-LD Core Context URL — latest supported version (v1.9).
pub const NGSI_LD_CORE_CONTEXT_V19_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld");
/// Core Context v1.9 embedded payload.
pub const NGSI_LD_CORE_CONTEXT_V19: &str = include_str!("context/ngsi-ld-core-context-v1.9.jsonld");

/// All known NGSI-LD core context URLs (aliases + versioned).
pub const NGSI_LD_CORE_CONTEXT_URL_LIST: [Iri<&'static str>; 8] = [
    NGSI_LD_CORE_CONTEXT_URL,
    NGSI_LD_CORE_CONTEXT_V13_URL,
    NGSI_LD_CORE_CONTEXT_V14_URL,
    NGSI_LD_CORE_CONTEXT_V15_URL,
    NGSI_LD_CORE_CONTEXT_V16_URL,
    NGSI_LD_CORE_CONTEXT_V17_URL,
    NGSI_LD_CORE_CONTEXT_V18_URL,
    NGSI_LD_CORE_CONTEXT_V19_URL,
];

/// Lookup table from core context URL to its embedded JSON payload.
///
/// This is written once at initialization and only read afterwards, so a
/// plain `HashMap` suffices (no concurrent-write support needed).
pub const NGSI_LD_CORE_CONTEXT_MAP: [(Iri<&'static str>, &str); 8] = [
    (NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V13),
    (NGSI_LD_CORE_CONTEXT_V14_URL, NGSI_LD_CORE_CONTEXT_V14),
    (NGSI_LD_CORE_CONTEXT_V15_URL, NGSI_LD_CORE_CONTEXT_V15),
    (NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V16),
    (NGSI_LD_CORE_CONTEXT_V17_URL, NGSI_LD_CORE_CONTEXT_V17),
    (NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_V18),
    (NGSI_LD_CORE_CONTEXT_V19_URL, NGSI_LD_CORE_CONTEXT_V19),
    (NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19),
];

/// Returns `true` if `url` matches any known NGSI-LD core context version.
pub fn is_ngsi_ld_core_context_url(url: Iri<&str>) -> bool {
    NGSI_LD_CORE_CONTEXT_URL_LIST.iter().any(|candidate| candidate == &url)
}

/// The embedded core-context document associated with a known URL.
pub fn embedded_core_context(url: Iri<&str>) -> Option<&'static str> {
    NGSI_LD_CORE_CONTEXT_MAP
        .iter()
        .find_map(|(candidate, payload)| (candidate == &url).then_some(*payload))
}

/// Whether two core context URLs resolve to the same embedded document.
///
/// The versionless alias and the versioned URL of the same release map to one
/// payload, so they are the same context; two different releases are not. This
/// is what the fastpath needs to decide whether an entity's inline core
/// reference agrees with the version the deployment pinned (clause 4.4):
/// only when it does may the fastpath's version-specific term table speak for
/// it. Payloads are compared by pointer, as each URL maps to one interned
/// `&'static str`.
pub fn same_core_context_document(a: Iri<&str>, b: Iri<&str>) -> bool {
    match (embedded_core_context(a), embedded_core_context(b)) {
        (Some(payload_a), Some(payload_b)) => ptr::eq(payload_a, payload_b),
        (Some(_) | None, _) => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_latest_core_context() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_V19_URL.as_ref()));
    }

    #[test]
    fn recognises_alias() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_URL.as_ref()));
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
