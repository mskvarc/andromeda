//! The term walk held to the processor.
//!
//! Every stored document of the corpus is compacted under every test context,
//! with and without `sysAttrs`, once by the walk and once as the read path
//! compacts it; the two compact entities must render to the same JSON,
//! and a document the processor refuses must be refused alike. The written
//! documents of the temporal and assembled paths are held to the processor the
//! same way, and single names to the terms their contexts spell them with.

mod contexts;
mod corpus;
mod documents;
mod names;
mod oracle;

use crate::{
    compacted::Compacted,
    differential_tests::contexts::{Decision, test_contexts},
    entity_compactor::{EntityCompactor, EntityRead},
    processor_fallback::{document_entity_through_processor, entity_through_processor, scope_history_through_processor, temporal_through_processor},
    state_reading::{WalkError, walk_entity},
    term_context::TermContext,
    walk::Walk,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument},
    entity_state::EntityState,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity::Entity, entity_system_times::EntitySystemTimes, temporal::temporal_document::TemporalDocument};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, TimeZone, Utc};
use indexmap::IndexMap;
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

/// Clause 4.5.24.2: `json` is "a raw JSON object (or array of objects)"
/// "never subject to JSON-LD term expansion or compaction", so the walk
/// returns an array literal as written -- one element, nested arrays and the
/// empty array included -- where the JSON-LD 1.1 add-value rule the processor
/// follows would add its elements one by one. The processor is therefore no
/// oracle for an array literal, and the corpus holds none.
#[tokio::test]
async fn a_json_property_array_is_returned_as_written() {
    let engine = offline_engine();
    let core = ContextSpec::core_only();
    let compactor = engine.compactor(&core);
    let typed = EntityCompactor::new(&compactor);
    let read = EntityRead {
        id_override: None,
        sys_attrs: SysAttrsInclusion::Strip,
        now: now(),
    };
    for written in [json!([{"id": "a"}]), json!([{"a": 1}, [1, 2], "text"]), json!([])] {
        let entity = json!({"id": "urn:ngsi-ld:Thing:tickets", "type": "Thing", "tickets": {"type": "JsonProperty", "json": written}});
        let expanded = oracle::expanded(&engine, &entity, &ContextSpec::none()).await;
        let state = state_of(&serde_json::to_value(&expanded).expect("the expansion renders"));
        let compacted = typed.entity(&state, &read).await.expect("the entity compacts");
        let compacted = serde_json::to_value(&compacted).expect("the compact entity renders");
        pretty_assertions::assert_eq!(compacted["tickets"], json!({"type": "JsonProperty", "json": written}), "{written}");
    }
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
            Walk::new(&terms)
                .compact_state(&state, SysAttrsInclusion::Include)
                .expect("the walk decides the core context"),
        );
        let read = <Entity>::from_node(&tree);
        assert!(read.is_ok(), "{}: {read:?}", document.name);
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

/// A Temporal Evolution read from the walk's tree is the one the processor's
/// compaction reads, under every context, or is refused alike.
#[tokio::test]
async fn every_written_document_reads_as_the_temporal_entity_the_processor_compacts_it_to_under_every_context() {
    let engine = offline_engine();
    let mut compared = 0_usize;
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
        for (name, expanded) in documents::written_documents() {
            let document =
                EntityDocument::from_slice(&serde_json::to_vec(&expanded).expect("the document renders")).expect("the canonical codec reads the document");
            let evolution = TemporalDocument::new(document.clone(), EntitySystemTimes::default(), IndexMap::new()); // clone: the oracle reads the same document
            let expected = temporal_through_processor(&compactor, &terms, &document)
                .await
                .map(|entity| format!("{entity:?}"))
                .map_err(|error| error.to_string());
            let actual = typed
                .temporal(&evolution)
                .await
                .map(|entity| format!("{entity:?}"))
                .map_err(|error| error.to_string());
            pretty_assertions::assert_eq!(actual, expected, "{name} under {}", context.name);
            compared += 1;
        }
    }
    assert!(compared > 0, "no document was compared");
}

/// The document holding the reified Scope instances under the Scope IRI, as a
/// temporal write expands them.
fn scope_history_document() -> EntityDocument {
    let scope: AttributeName = "https://uri.etsi.org/ngsi-ld/scope".parse().expect("an attribute name");
    let instances = documents::scope_history_instances()
        .iter()
        .map(|instance| InstanceDocument::from_slice(&scope, &serde_json::to_vec(instance).expect("the instance renders")).expect("the instance decodes"))
        .collect();
    EntityDocument::new(None, Vec::new(), None, None, IndexMap::from([(scope, AttributeMember::Instances(instances))]))
}

/// The reified Scope history read from the walk's tree is the one the
/// processor's compaction reads, under every context: those the walk decides
/// and those it leaves to the processor.
#[tokio::test]
async fn a_scope_history_reads_as_the_processor_compacts_it_under_every_context() {
    let engine = offline_engine();
    let document = scope_history_document();
    let mut compared = 0_usize;
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
        let expected = scope_history_through_processor(&compactor, &terms, &document)
            .await
            .map(|history| format!("{history:?}"))
            .map_err(|error| error.to_string());
        let actual = typed
            .scope_history(&document)
            .await
            .map(|history| format!("{history:?}"))
            .map_err(|error| error.to_string());
        pretty_assertions::assert_eq!(actual, expected, "under {}", context.name);
        compared += 1;
    }
    assert!(compared > 0, "no context was compared");
}

/// Clause 4.5.6: whichever way a context is compacted, both Scope instances
/// are read with their temporal members; the processor fallback reads them from
/// the compacted bytes.
#[tokio::test]
async fn the_processor_fallback_reads_both_instances_of_a_scope_history() {
    let engine = offline_engine();
    let document = scope_history_document();
    let core = test_contexts()
        .into_iter()
        .find(|context| matches!(context.decision, Decision::Walk))
        .expect("a context the walk decides");
    let compactor = engine.compactor(&core.spec);
    let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
    let history = scope_history_through_processor(&compactor, &terms, &document)
        .await
        .expect("the history compacts")
        .expect("the document holds a Scope history");
    pretty_assertions::assert_eq!(history.instances().len(), 2);
    let rendered = serde_json::to_value(history.into_member()).expect("the history renders");
    pretty_assertions::assert_eq!(rendered[0]["value"], json!("/Madrid/Gardens"));
    pretty_assertions::assert_eq!(rendered[0]["createdAt"], json!("2025-01-01T00:00:01Z"));
    pretty_assertions::assert_eq!(rendered[1]["deletedAt"], json!("2025-01-02T00:00:00Z"));
}

/// A document holding no Scope history gives none.
#[tokio::test]
async fn a_document_without_a_scope_history_gives_none() {
    let engine = offline_engine();
    let spec = ContextSpec::core_only();
    let compactor = engine.compactor(&spec);
    let document = EntityDocument::default();
    let history = EntityCompactor::new(&compactor).scope_history(&document).await.expect("the document compacts");
    assert!(history.is_none());
}

#[tokio::test]
async fn every_written_document_reads_as_the_entity_the_processor_compacts_it_to_under_every_context() {
    let engine = offline_engine();
    for context in test_contexts() {
        let compactor = engine.compactor(&context.spec);
        let typed = EntityCompactor::new(&compactor);
        let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
        for (name, expanded) in documents::written_documents() {
            let document =
                EntityDocument::from_slice(&serde_json::to_vec(&expanded).expect("the document renders")).expect("the canonical codec reads the document");
            let expected = rendered(
                document_entity_through_processor(&compactor, &terms, &document)
                    .await
                    .map_err(|error| error.to_string()),
            );
            let actual = rendered(typed.document_entity(&document).await.map_err(|error| error.to_string()));
            pretty_assertions::assert_eq!(actual, expected, "{name} under {}", context.name);
        }
    }
}

/// The fallback hands the processor the canonical bytes of the stored state:
/// a type-scoped context leaves the walk undecided (`Vehicle` scopes `speed`),
/// and the entity the fallback reads from those bytes is the one the processor
/// gives for the stored document.
#[tokio::test]
async fn the_processor_fallback_reads_bytes() {
    let engine = offline_engine();
    let scoped = test_contexts()
        .into_iter()
        .find(|context| context.name == "type-scoped")
        .expect("the type-scoped context");
    let compactor = engine.compactor(&scoped.spec);
    let terms = TermContext::new(compactor.processed_context().await.expect("the context processes"));
    let corpus = corpus::stored_documents(&engine).await;
    let vehicle = corpus
        .iter()
        .find(|document| document.name == "http-identifiers")
        .expect("the corpus holds a Vehicle");
    let state = state_of(&vehicle.expanded);

    assert!(matches!(
        walk_entity(&terms, &state, None, SysAttrsInclusion::Include),
        Err(WalkError::Undecidable(_))
    ));
    let fallback = entity_through_processor(&compactor, &terms, &state, None, SysAttrsInclusion::Include)
        .await
        .map_err(|error| error.to_string());
    let expected = oracle::processor_entity(&compactor, &vehicle.expanded, None, SysAttrsInclusion::Include, now()).await;
    pretty_assertions::assert_eq!(rendered(fallback), rendered(expected));
}
