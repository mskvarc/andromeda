//! The registrations and entities the batch forwarding tests share.

use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use indexmap::IndexMap;
use serde_json::json;

/// Parse a fixture identifier.
pub(super) fn uri(raw: &str) -> Identifier {
    raw.parse().expect("valid URI")
}

/// A registration of `mode` covering the entity selector and properties
/// given, advertising `operations`.
pub(super) fn registration(
    id: &str,
    mode: &str,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
    properties: &[&str],
    operations: &[&str],
) -> ContextSourceRegistration {
    let mut selector = json!({});
    if let Some(entity_type) = entity_type {
        selector["type"] = json!(entity_type);
    }
    if let Some(entity_id) = entity_id {
        selector["id"] = json!(entity_id);
    }
    let mut information = json!({});
    if entity_type.is_some() || entity_id.is_some() {
        information["entities"] = json!([selector]);
    }
    if !properties.is_empty() {
        information["propertyNames"] = json!(properties);
    }
    serde_json::from_value(json!({
    "id": id,
    "type": "ContextSourceRegistration",
    "endpoint": "https://peer.example.org",
    "mode": mode,
    "information": [information],
    "operations": operations,
    "status": "ok",
    "createdAt": "2024-01-01T00:00:00Z",
    "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}

/// The document the admission expands an entity to: `id`, one type and one
/// empty instance of every attribute IRI of `attributes`.
pub(super) fn document(id: &str, entity_type: &str, attributes: &[&str]) -> EntityDocument {
    EntityDocument::new(
        Some(uri(id)),
        vec![entity_type.parse().expect("a valid type name")],
        None,
        None,
        attributes
            .iter()
            .map(|iri| {
                let name: AttributeName = iri.parse().expect("a valid attribute name");
                (name, AttributeMember::Instances(vec![InstanceDocument::builder().build()]))
            })
            .collect::<IndexMap<_, _>>(),
    )
}
