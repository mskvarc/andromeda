//! The compound `@context` of the ETSI NGSI-LD test suite: the v1.9 Core
//! @context followed by the suite's own terms under its `@vocab`.

/// The ETSI test suite compound `@context` document.
pub const ETSI_TEST_SUITE_COMPOUND_CONTEXT: &str = include_str!("../context/ngsi-ld-test-suite-compound.jsonld");

#[cfg(test)]
mod tests {
    use crate::{NGSI_LD_CORE_CONTEXT_URL, test_suite_context::ETSI_TEST_SUITE_COMPOUND_CONTEXT};

    #[test]
    fn the_test_suite_compound_context_opens_with_the_v1_9_core_context() {
        let document: serde_json::Value = serde_json::from_str(ETSI_TEST_SUITE_COMPOUND_CONTEXT).expect("the document is JSON");
        pretty_assertions::assert_eq!(document["@context"][0], NGSI_LD_CORE_CONTEXT_URL.as_str());
        pretty_assertions::assert_eq!(document["@context"][1]["@vocab"], "https://ngsi-ld-test-suite/context#");
    }
}
