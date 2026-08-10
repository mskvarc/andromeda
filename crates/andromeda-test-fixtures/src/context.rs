//! Canonical JSON-LD context IRIs reused by integration suites.

use iri_rs::{Iri, iri};

/// NGSI-LD v1.9 Core Context.
pub const CORE_CONTEXT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld");

/// ETSI NGSI-LD test-suite context.
pub const TEST_SUITE_CONTEXT: &Iri<&'static str> =
    &iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld");

/// ETSI NGSI-LD compound test-suite context.
pub const COMPOUND_CONTEXT: &Iri<&'static str> =
    &iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld");
