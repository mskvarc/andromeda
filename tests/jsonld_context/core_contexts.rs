//! The NGSI-LD core `@context` releases a client may name.
//!
//! Clause 4.4 names one core `@context`, v1.9; Andromeda accepts every
//! embedded release and the versionless alias as a name for it, and only ever
//! names v1.9 itself.

use andromeda_test_fixtures::context::CORE_CONTEXT;
use serde_json::{Value, json};

/// The v1.9 core `@context`, the only one a response names.
pub fn core() -> &'static str {
    CORE_CONTEXT.as_str()
}

/// The v1.9 core `@context` as the plain string a rendered `@context` holds.
pub fn core_value() -> Value {
    json!(core())
}

/// The v1.8 core `@context`, protected and differing from v1.9 on three terms.
pub const CORE_V18: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// The v1.6 core `@context`.
pub const CORE_V16: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld";

/// The v1.3 core `@context`, the oldest embedded release.
pub const CORE_V13: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld";

/// The versionless alias, accepted as the core `@context` and never emitted.
pub const CORE_VERSIONLESS: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld";
