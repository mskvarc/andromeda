//! NGSI-LD core context URL constants.
//!
//! Provides canonical URLs for all released versions of the NGSI-LD core
//! context (ETSI CIM 009) and a helper to test whether a URL matches any
//! known version.

use dashmap::DashMap;
use std::sync::LazyLock;

/// Core Context v1.3.
pub const NGSI_LD_CORE_CONTEXT_V13_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V13: &str = include_str!("context/ngsi-ld-core-context-v1.3.jsonld");

/// Core Context v1.4.
pub const NGSI_LD_CORE_CONTEXT_V14_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V14: &str = include_str!("context/ngsi-ld-core-context-v1.4.jsonld");

/// Core Context v1.5.
pub const NGSI_LD_CORE_CONTEXT_V15_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V15: &str = include_str!("context/ngsi-ld-core-context-v1.5.jsonld");

/// Core Context v1.6.
pub const NGSI_LD_CORE_CONTEXT_V16_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V16: &str = include_str!("context/ngsi-ld-core-context-v1.6.jsonld");

/// Core Context v1.7.
pub const NGSI_LD_CORE_CONTEXT_V17_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V17: &str = include_str!("context/ngsi-ld-core-context-v1.7.jsonld");

/// Core Context v1.8.
pub const NGSI_LD_CORE_CONTEXT_V18_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V18: &str = include_str!("context/ngsi-ld-core-context-v1.8.jsonld");

/// NGSI-LD Core Context URL — latest supported version (v1.9.1).
pub const NGSI_LD_CORE_CONTEXT_V19_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
pub const NGSI_LD_CORE_CONTEXT_V19: &str = include_str!("context/ngsi-ld-core-context-v1.9.jsonld");

/// Version-less alias that resolves to the latest core context.
pub const NGSI_LD_CORE_CONTEXT_URL: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld";

/// All known NGSI-LD core context URLs (aliases + versioned).
pub const NGSI_LD_CORE_CONTEXT_URL_LIST: [&str; 8] = [
    NGSI_LD_CORE_CONTEXT_URL,
    NGSI_LD_CORE_CONTEXT_V13_URL,
    NGSI_LD_CORE_CONTEXT_V14_URL,
    NGSI_LD_CORE_CONTEXT_V15_URL,
    NGSI_LD_CORE_CONTEXT_V16_URL,
    NGSI_LD_CORE_CONTEXT_V17_URL,
    NGSI_LD_CORE_CONTEXT_V18_URL,
    NGSI_LD_CORE_CONTEXT_V19_URL,
];

pub static NGSI_LD_CORE_CONTEXT_MAP: LazyLock<DashMap<&str, &str>> = LazyLock::new(|| {
    let m = DashMap::new();
    m.insert(NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V13);
    m.insert(NGSI_LD_CORE_CONTEXT_V14_URL, NGSI_LD_CORE_CONTEXT_V14);
    m.insert(NGSI_LD_CORE_CONTEXT_V15_URL, NGSI_LD_CORE_CONTEXT_V15);
    m.insert(NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V16);
    m.insert(NGSI_LD_CORE_CONTEXT_V17_URL, NGSI_LD_CORE_CONTEXT_V17);
    m.insert(NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_V18);
    m.insert(NGSI_LD_CORE_CONTEXT_V19_URL, NGSI_LD_CORE_CONTEXT_V19);
    m.insert(NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19);

    m
});

/// W3C Data Integrity context URL.
pub const DATA_INTEGRITY_CONTEXT_URL: &str = "https://w3id.org/security/data-integrity/v2";
pub const DATA_INTEGRITY_CONTEXT: &str = include_str!("context/data-integrity-v2.jsonld");

/// Returns `true` if `url` matches any known NGSI-LD core context version.
pub fn is_ngsi_ld_core_context_url(url: &str) -> bool {
    NGSI_LD_CORE_CONTEXT_URL_LIST.iter().any(|core| *core == url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_latest_core_context() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_V19_URL));
    }

    #[test]
    fn recognises_alias() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_URL));
    }

    #[test]
    fn recognises_v16() {
        assert!(is_ngsi_ld_core_context_url(NGSI_LD_CORE_CONTEXT_V16_URL));
    }

    #[test]
    fn rejects_unknown_url() {
        assert!(!is_ngsi_ld_core_context_url("https://example.com/context.jsonld"));
    }
}
