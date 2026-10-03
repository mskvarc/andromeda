//! The term walk held to the processor.
//!
//! Every stored document of the corpus is compacted under every test context,
//! with and without `sysAttrs`, once by the walk and once as the read path has
//! always compacted it; the two compact entities must render to the same JSON,
//! and a document the processor refuses must be refused alike. The written
//! documents of the temporal and assembled paths are held to the processor the
//! same way, and single names to the terms their contexts spell them with.

mod contexts;
mod corpus;
mod documents;
mod oracle;

use crate::{
    compacted::Compacted,
    differential_tests::contexts::{Decision, test_contexts},
    entity_compactor::{EntityCompactor, EntityRead},
    name_compaction::NameCompaction,
    state_reading::{WalkError, reads_without_document, walk_entity},
    term_context::TermContext,
    walk::{GeometryOutput, Walk},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Value, json};

/// An engine that resolves only what is embedded, never the network.
fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds")
}

/// The instant every read is taken at: after the corpus's expired instances,
/// before the ones that stay.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("a valid instant")
}

/// The state a stored document decodes into.
fn state_of(stored: &Value) -> EntityState {
    EntityState::from_slice(&serde_json::to_vec(stored).expect("the stored document renders")).expect("the canonical codec reads the stored document")
}

/// A compaction outcome rendered for comparison: the compact entity's JSON,
/// or the failure's text.
fn rendered(outcome: Result<Entity, String>) -> Result<String, String> {
    outcome.map(|entity| serde_json::to_string(&entity).expect("the compact entity renders"))
}

#[tokio::test]
async fn every_stored_document_compacts_as_the_processor_compacts_it_under_every_context() {
    let engine = offline_engine();
    let corpus = corpus::stored_documents(&engine).await;
    let mut compared = 0_usize;
    let mut compacted = 0_usize;
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        for document in &corpus {
            let state = state_of(&document.expanded);
            for sys_attrs in [SysAttrsInclusion::Include, SysAttrsInclusion::Strip] {
                let read = EntityRead {
                    id_override: None,
                    sys_attrs,
                    now: now(),
                };
                let expected = rendered(oracle::processor_entity(&compactor, &document.expanded, None, sys_attrs, now()).await);
                let actual = rendered(typed.entity(&state, &read).await.map_err(|error| error.to_string()));
                compacted += usize::from(actual.is_ok());
                pretty_assertions::assert_eq!(actual, expected, "{} under {} with {sys_attrs:?}", document.name, context.name);
                compared += 1;
            }
        }
    }
    assert!(compared > 500, "only {compared} comparisons ran");
    assert!(compacted > 400, "only {compacted} of {compared} comparisons compacted an entity");
}

/// Clause 3.1: a Property holding the empty array, stored as `hasValue: []`,
/// renders `"value": []` (JSON-LD 1.1 compaction keeps an empty array), at
/// the top level and inside a structured value.
#[tokio::test]
async fn a_property_holding_the_empty_array_renders_the_empty_array() {
    let engine = offline_engine();
    let core = ContextSpec::core_only();
    let compactor = engine.compactor(&core);
    let typed = EntityCompactor::new(&compactor);
    let read = EntityRead {
        id_override: None,
        sys_attrs: SysAttrsInclusion::Strip,
        now: now(),
    };
    let corpus = corpus::stored_documents(&engine).await;
    let entity = |name: &str| {
        corpus
            .iter()
            .find(|document| document.name == name)
            .unwrap_or_else(|| panic!("the corpus holds {name}"))
    };
    let every_family = typed
        .entity(&state_of(&entity("every-family").expanded), &read)
        .await
        .expect("the entity compacts");
    let structured = typed
        .entity(&state_of(&entity("structured-values").expanded), &read)
        .await
        .expect("the entity compacts");

    let every_family = serde_json::to_value(&every_family).expect("the compact entity renders");
    let structured = serde_json::to_value(&structured).expect("the compact entity renders");
    pretty_assertions::assert_eq!(every_family["untagged"], json!({"type": "Property", "value": []}));
    pretty_assertions::assert_eq!(structured["address"]["value"]["flats"], json!([]));
}

/// How many geometries `node` keeps typed.
fn typed_geometries(node: &Compacted<'_>) -> usize {
    match node {
        Compacted::Geometry(_) => 1,
        Compacted::Array(items) => items.iter().map(typed_geometries).sum(),
        Compacted::Object(members) => members.iter().map(|(_, member)| typed_geometries(member)).sum(),
        Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) => 0,
    }
}

/// Under the core context every stored document is read straight from the
/// walk's tree, every typed-readable geometry kept typed, without a JSON
/// document of the entity being written.
#[tokio::test]
async fn every_document_is_read_from_the_walk_with_its_geometries_kept_typed_under_the_core_context() {
    let engine = offline_engine();
    let core = ContextSpec::core_only();
    let compactor = engine.compactor(&core);
    let terms = TermContext::new(compactor.processed_context().await.expect("the core context processes"));
    let mut kept = 0_usize;
    for document in corpus::stored_documents(&engine).await {
        let state = state_of(&document.expanded);
        let tree = Compacted::Object(
            Walk::new(&terms, GeometryOutput::Typed)
                .compact_state(&state, SysAttrsInclusion::Include)
                .expect("the walk decides the core context"),
        );
        assert!(reads_without_document(&terms, &tree), "{}", document.name);
        assert!(Entity::from_node(&tree).is_ok(), "{}", document.name);
        kept += typed_geometries(&tree);
    }
    pretty_assertions::assert_eq!(kept, 10);
}

#[tokio::test]
async fn the_walk_decides_every_document_of_a_context_without_scoped_terms_nests_or_map_containers() {
    let engine = offline_engine();
    let corpus = corpus::stored_documents(&engine).await;
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
        let undecided: Vec<&str> = corpus
            .iter()
            .filter(|document| {
                matches!(
                    walk_entity(&terms, &state_of(&document.expanded), None, SysAttrsInclusion::Include),
                    Err(WalkError::Undecidable(_))
                )
            })
            .map(|document| document.name)
            .collect();
        match context.decision {
            Decision::Walk => assert!(undecided.is_empty(), "{} left {undecided:?} to the processor", context.name),
            Decision::Processor => assert!(!undecided.is_empty(), "{} left nothing to the processor", context.name),
        }
    }
}

#[tokio::test]
async fn an_entity_read_by_another_identifier_is_named_after_it_by_both_readings() {
    let engine = offline_engine();
    let corpus = corpus::stored_documents(&engine).await;
    let id: Identifier = "urn:ngsi-ld:Vehicle:read-as".parse().expect("an identifier");
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        for document in &corpus {
            let read = EntityRead {
                id_override: Some(&id),
                sys_attrs: SysAttrsInclusion::Strip,
                now: now(),
            };
            let expected = rendered(oracle::processor_entity(&compactor, &document.expanded, Some(id.as_str()), SysAttrsInclusion::Strip, now()).await);
            let actual = rendered(typed.entity(&state_of(&document.expanded), &read).await.map_err(|error| error.to_string()));
            pretty_assertions::assert_eq!(actual, expected, "{} under {}", document.name, context.name);
        }
    }
}

#[tokio::test]
async fn every_written_document_compacts_as_the_processor_compacts_it_under_every_context() {
    let engine = offline_engine();
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        for (name, expanded) in documents::written_documents() {
            let document =
                EntityDocument::from_slice(&serde_json::to_vec(&expanded).expect("the document renders")).expect("the canonical codec reads the document");
            let expected = oracle::processor_document(&compactor, &expanded).await;
            let actual = typed.document(&document).await.map_err(|error| error.to_string());
            pretty_assertions::assert_eq!(actual, expected, "{name} under {}", context.name);
        }
    }
}

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
