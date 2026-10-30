//! The deletion each family's written value compacts to, as a relayed
//! notification and a merge write it.

use crate::{differential_tests::documents, entity_compactor::EntityCompactor};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use serde_json::json;

/// Clauses 4.5.0, 4.5.18.2, 4.5.20.2, 4.5.22.2 and 4.5.24.2: a written
/// document deleting a whole value compacts to its family's deletion
/// encoding, a language map to the one holding the Null under `@none` and an
/// object list to the array of the one Null.
#[tokio::test]
async fn a_written_deletion_of_each_value_compacts_to_its_family_encoding() {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let core = ContextSpec::core_only();
    let compactor = engine.compactor(&core);
    let (_, expanded) = documents::written_documents()
        .into_iter()
        .find(|(name, _)| *name == "value-deletions")
        .expect("the value deletions are written");
    let document = EntityDocument::from_slice(&serde_json::to_vec(&expanded).expect("the document renders")).expect("the canonical codec reads the document");
    let entity = EntityCompactor::new(&compactor)
        .document_entity(&document)
        .await
        .expect("the document compacts");
    let compacted = serde_json::to_value(&entity).expect("the compact entity renders");
    pretty_assertions::assert_eq!(compacted["removed"], json!("urn:ngsi-ld:null"));
    pretty_assertions::assert_eq!(compacted["speed"], json!({"type": "Property", "value": "urn:ngsi-ld:null"}));
    pretty_assertions::assert_eq!(compacted["owner"], json!({"type": "Relationship", "object": "urn:ngsi-ld:null"}));
    pretty_assertions::assert_eq!(
        compacted["name"],
        json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}})
    );
    pretty_assertions::assert_eq!(compacted["rooms"], json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]}));
    pretty_assertions::assert_eq!(compacted["readings"], json!({"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"]}));
    pretty_assertions::assert_eq!(compacted["category"], json!({"type": "VocabProperty", "vocab": "urn:ngsi-ld:null"}));
    pretty_assertions::assert_eq!(compacted["config"], json!({"type": "JsonProperty", "json": "urn:ngsi-ld:null"}));
}
