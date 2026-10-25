//! Compacting a registration's `information` entries (Table 5.2.9-1).
//!
//! Entity types and attribute names are stored expanded, so every
//! representation of a registration compacts them back through the @context it
//! is rendered under: the discovery response of clause 5.10.2.5, the retrieval
//! of clause 5.10.4.5 and the Context Source Notification of clause 5.11.7
//! alike. One reading, one implementation -- a client that discovers a source
//! and is then notified about it must be handed the same names both times.

use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    entity_info::{EntityInfo, EntityTypeSelection},
    registration::info::RegistrationInfo,
};
use std::str::FromStr;

/// The `information` entries, their entity types and attribute names compacted
/// with the operation's one [`NameCompaction`].
///
/// A term the context compacts a name to is used where it is a name NGSI-LD
/// can carry; one that is not -- a context may define any string as a term --
/// leaves the name in the expanded form it is stored in, which names the same
/// thing.
pub fn compacted_information<'i>(names: &NameCompaction, information: impl IntoIterator<Item = &'i RegistrationInfo>) -> Vec<RegistrationInfo> {
    information
        .into_iter()
        .map(|entry| RegistrationInfo {
            entities: entry
                .entities
                .as_ref()
                .map(|entities| entities.iter().map(|entity| compacted_entity(names, entity)).collect()),
            property_names: entry.property_names.as_ref().map(|attributes| compacted_attributes(names, attributes)),
            relationship_names: entry.relationship_names.as_ref().map(|attributes| compacted_attributes(names, attributes)),
        })
        .collect()
}

/// An `EntityInfo`, its `type` compacted in whichever of its two shapes it was
/// stored (Table 5.2.8-1), keeping that shape.
fn compacted_entity(names: &NameCompaction, entity: &EntityInfo) -> EntityInfo {
    let type_ = match &entity.type_ {
        EntityTypeSelection::Single(name) => EntityTypeSelection::Single(compacted_type(names, name)),
        EntityTypeSelection::Several(several) => EntityTypeSelection::Several(several.iter().map(|name| compacted_type(names, name)).collect()),
    };
    EntityInfo {
        type_,
        id: entity.id.clone(),                 // clone: the compacted entry owns its copy of the stored identifier
        id_pattern: entity.id_pattern.clone(), // clone: the compacted entry owns its copy of the stored pattern
    }
}

/// One entity type, compacted.
fn compacted_type(names: &NameCompaction, name: &EntityTypeName) -> EntityTypeName {
    // clone: a term that is no entity type name leaves the stored name in place
    EntityTypeName::from_str(&names.compact_type_iri(name.as_str())).unwrap_or_else(|_not_a_name| name.clone())
}

/// One of the two attribute-name arrays a `RegistrationInfo` carries,
/// compacted.
fn compacted_attributes(names: &NameCompaction, attributes: &[AttributeName]) -> Vec<AttributeName> {
    attributes
        .iter()
        // clone: a term that is no attribute name leaves the stored name in place
        .map(|name| AttributeName::from_str(&names.compact_attr_iri(name.as_str())).unwrap_or_else(|_not_a_name| name.clone()))
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::information_compaction::compacted_information;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use serde_json::json;

    /// Names compacted under the core context, resolved offline.
    async fn core_names() -> NameCompaction {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds");
        NameCompaction::new(
            engine
                .compactor(&ContextSpec::none())
                .processed_context()
                .await
                .expect("the core context processes"),
        )
    }

    fn information(document: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(document).expect("a RegistrationInfo")
    }

    /// Clause 5.10.4.5: entity types and attribute names in the default
    /// vocabulary compact to their terms, and an IRI no term names stays
    /// expanded; each `type` keeps the shape it was stored in.
    #[tokio::test]
    async fn names_compact_under_the_context_and_types_keep_their_shape() {
        let stored = [
            information(json!({
                "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle", "id": "urn:ngsi-ld:Vehicle:1"}],
                "propertyNames": ["https://uri.etsi.org/ngsi-ld/default-context/speed", "https://example.com/brand"]
            })),
            information(json!({
                "entities": [{"type": ["https://uri.etsi.org/ngsi-ld/default-context/Car", "https://example.com/Bus"], "idPattern": "urn:ngsi-ld:Car:.*"}],
                "relationshipNames": ["https://uri.etsi.org/ngsi-ld/default-context/owner"]
            })),
        ];

        let compacted = compacted_information(&core_names().await, &stored);

        pretty_assertions::assert_eq!(
            serde_json::to_value(&compacted).expect("serialises"),
            json!([
                {"entities": [{"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"}], "propertyNames": ["speed", "https://example.com/brand"]},
                {"entities": [{"type": ["Car", "https://example.com/Bus"], "idPattern": "urn:ngsi-ld:Car:.*"}], "relationshipNames": ["owner"]}
            ])
        );
    }
}
