//! Single names held to the terms their contexts spell them with.

use crate::{
    differential_tests::{contexts::test_contexts, offline_engine},
    name_compaction::NameCompaction,
};

/// The names every name compaction test asks for: default-context terms,
/// names a test context declares, core terms, the ETSI test-suite vocabulary,
/// a name nothing declares, and names outside the schemes an expanded NGSI-LD
/// name is written in.
const NAMES: [&str; 16] = [
    "https://uri.etsi.org/ngsi-ld/default-context/speed",
    "https://uri.etsi.org/ngsi-ld/default-context/Vehicle",
    "https://uri.etsi.org/ngsi-ld/default-context/brandName",
    "https://uri.etsi.org/ngsi-ld/default-context/count",
    "https://uri.etsi.org/ngsi-ld/default-context/isParked",
    "http://example.org/vehicle/brandName",
    "http://example.org/vehicle/Car",
    "https://uri.etsi.org/ngsi-ld/Property",
    "https://uri.etsi.org/ngsi-ld/observedAt",
    "https://uri.etsi.org/ngsi-ld/location",
    "https://ngsi-ld-test-suite/context#speed",
    "https://example.com/vocab#unknownTerm",
    "urn:ngsi-ld:Thing:name",
    "did:example:thing",
    "ex:Vehicle",
    "Vehicle",
];

/// What each of [`NAMES`] compacts to under a test context, in order.
const EXPECTED_NAMES: [(&str, [&str; 16]); 5] = [
    (
        "none",
        [
            "speed",
            "Vehicle",
            "brandName",
            "count",
            "isParked",
            "http://example.org/vehicle/brandName",
            "http://example.org/vehicle/Car",
            "Property",
            "ngsi-ld:observedAt",
            "location",
            "https://ngsi-ld-test-suite/context#speed",
            "https://example.com/vocab#unknownTerm",
            "urn:ngsi-ld:Thing:name",
            "did:example:thing",
            "ex:Vehicle",
            "Vehicle",
        ],
    ),
    (
        "terms",
        [
            "speed",
            "Vehicle",
            "ngsi-ld:default-context/brandName",
            "count",
            "isParked",
            "brandName",
            "http://example.org/vehicle/Car",
            "Property",
            "ngsi-ld:observedAt",
            "location",
            "https://ngsi-ld-test-suite/context#speed",
            "https://example.com/vocab#unknownTerm",
            "urn:ngsi-ld:Thing:name",
            "did:example:thing",
            "ex:Vehicle",
            "Vehicle",
        ],
    ),
    (
        "vocab",
        [
            "speed",
            "Vehicle",
            "brandName",
            "count",
            "isParked",
            "http://example.org/vehicle/brandName",
            "http://example.org/vehicle/Car",
            "Property",
            "ngsi-ld:observedAt",
            "location",
            "https://ngsi-ld-test-suite/context#speed",
            "https://example.com/vocab#unknownTerm",
            "urn:ngsi-ld:Thing:name",
            "did:example:thing",
            "ex:Vehicle",
            "Vehicle",
        ],
    ),
    (
        "prefixes",
        [
            "speed",
            "Vehicle",
            "brandName",
            "count",
            "isParked",
            "ex:brandName",
            "ex:Car",
            "Property",
            "ngsi-ld:observedAt",
            "location",
            "https://ngsi-ld-test-suite/context#speed",
            "https://example.com/vocab#unknownTerm",
            "urn:ngsi-ld:Thing:name",
            "did:example:thing",
            "ex:Vehicle",
            "Vehicle",
        ],
    ),
    (
        "datatype-coerced",
        [
            "speed",
            "Vehicle",
            "brandName",
            "ngsi-ld:default-context/count",
            "isParked",
            "http://example.org/vehicle/brandName",
            "http://example.org/vehicle/Car",
            "Property",
            "ngsi-ld:observedAt",
            "location",
            "https://ngsi-ld-test-suite/context#speed",
            "https://example.com/vocab#unknownTerm",
            "urn:ngsi-ld:Thing:name",
            "did:example:thing",
            "ex:Vehicle",
            "Vehicle",
        ],
    ),
];

/// Clause 5.5.7 and Table 5.2.28-1: each name is the term the context spells
/// it with -- a vocabulary-relative term, a declared term, a compact IRI where
/// a term coercing its values keeps the shorter spelling from the name -- or,
/// where nothing abbreviates it, the name itself (clause 4.5.1). An attribute
/// name and an entity type name compact alike.
#[tokio::test]
async fn every_name_compacts_to_the_term_its_context_spells_it_with() {
    let engine = offline_engine();
    let contexts = test_contexts();
    let mut actual = Vec::new();
    for (context_name, _) in EXPECTED_NAMES {
        let context = contexts
            .iter()
            .find(|context| context.name == context_name)
            .expect("the expected names name a test context");
        let processed = engine.compactor(&context.spec).processed_context().await.expect("the context processes");
        let names = NameCompaction::new(processed);
        let attributes: Vec<String> = NAMES.iter().map(|name| names.compact_attr_iri(name).to_string()).collect();
        let types: Vec<String> = NAMES.iter().map(|name| names.compact_type_iri(name).to_string()).collect();
        pretty_assertions::assert_eq!(attributes, types, "attribute and type names compact alike under {context_name}");
        actual.push((context_name, attributes));
    }
    let expected: Vec<(&str, Vec<String>)> = EXPECTED_NAMES
        .iter()
        .map(|(context_name, names)| (*context_name, names.iter().map(|name| (*name).to_owned()).collect()))
        .collect();
    pretty_assertions::assert_eq!(actual, expected);
}

/// Every test context that processes compacts every name to a term or to the
/// name itself, the same each time it is asked.
#[tokio::test]
async fn every_name_compacts_alike_each_time_under_every_context() {
    let engine = offline_engine();
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let Ok(processed) = compactor.processed_context().await else {
            continue;
        };
        let names = NameCompaction::new(processed);
        for name in NAMES {
            let first = names.compact_attr_iri(name);
            assert!(!first.is_empty(), "{name} under {}", context.name);
            pretty_assertions::assert_eq!(names.compact_type_iri(name), first, "{name} under {}", context.name);
        }
    }
}
