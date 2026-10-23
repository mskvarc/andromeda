//! A store read under a context whose terms alias core terms or reshape what
//! is compacted under them reads the stored entity typed, and the typed
//! entity is written with the core terms alone (clauses 4.4 and 4.5.0).

use crate::{
    differential_tests::{contexts::test_contexts, now, offline_engine, state_of},
    entity_compactor::{EntityCompactor, EntityRead},
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::{Value, json};

/// A stored Vehicle whose `speed` is a Property of 50.
fn stored_vehicle() -> EntityState {
    state_of(&json!([{
        "@id": "urn:ngsi-ld:Vehicle:1",
        "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
        "https://uri.etsi.org/ngsi-ld/default-context/speed": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 50}]
        }]
    }]))
}

/// The stored Vehicle read under the test context `name`, rendered.
async fn vehicle_read_under(engine: &JsonLdEngine, name: &str) -> Value {
    let context = test_contexts()
        .into_iter()
        .find(|context| context.name == name)
        .expect("the test context exists");
    let compactor = engine.compactor(&context.spec);
    let read = EntityRead {
        id_override: None,
        sys_attrs: SysAttrsInclusion::Strip,
        now: now(),
    };
    let entity = EntityCompactor::new(&compactor)
        .entity(&stored_vehicle(), &read)
        .await
        .unwrap_or_else(|error| panic!("the Vehicle reads typed under {name}: {error}"));
    serde_json::to_value(&entity).expect("the compact entity renders")
}

/// Clauses 4.4 and 4.5.0: a context aliasing `@type` or `ngsi-ld:hasValue`
/// makes compaction select the alias, the entity still reads typed, and the
/// typed entity is written with the core terms alone.
#[tokio::test]
async fn a_store_read_under_an_aliasing_context_reads_typed() {
    let engine = offline_engine();
    for name in ["value-hijack", "set-container"] {
        let rendered = vehicle_read_under(&engine, name).await;
        pretty_assertions::assert_eq!(rendered["id"], json!("urn:ngsi-ld:Vehicle:1"), "{name}: {rendered}");
        pretty_assertions::assert_eq!(rendered["type"], json!("Vehicle"), "{name}: {rendered}");
        pretty_assertions::assert_eq!(rendered["speed"], json!({"type": "Property", "value": 50}), "{name}: {rendered}");
        assert!(rendered.get("tps").is_none() && rendered["speed"].get("v").is_none(), "{name}: {rendered}");
    }
}

/// A context the walk leaves to the processor (an `@index` container on an
/// attribute term) still reads the entity typed.
#[tokio::test]
async fn a_store_read_through_the_processor_reads_typed() {
    let rendered = vehicle_read_under(&offline_engine(), "index-container").await;
    pretty_assertions::assert_eq!(rendered["type"], json!("Vehicle"), "{rendered}");
    pretty_assertions::assert_eq!(rendered["speed"]["value"], json!(50), "{rendered}");
}
