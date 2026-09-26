//! The representation of one entity in a notification (clause 5.8.6):
//! compaction under the subscription's `@context` and the
//! `notification.format` rendering.
//!
//! Shared by the change-driven and the periodic notification paths, which
//! build their entities differently but deliver them the same way.

use crate::processor::payload::{
    cache::{NotificationCompactionCache, compact_entity_with_cache},
    context::normalize_compacted_entity,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{context_compactor::ContextCompactor, options::ContextStripping},
    error::JsonLdError,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_render::{concise::concise_value, object_list::normalize_object_lists, simplify::simplify_entity, snapshot::SnapshotRenderer};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde_json::Value;
use tracing::debug;

/// Compact an expanded entity under the subscription's context, the one
/// `compactor` compacts against, falling back to the core context when that
/// context cannot be applied.
///
/// The `@context` is always stripped from the entity: a notification carries
/// one context for the whole document (clauses 5.2.13 and 6.3.5), which the
/// delivery request adds when the media type is `application/ld+json`.
///
/// # Errors
///
/// Returns the core-context failure when neither context can compact the
/// entity, which leaves nothing to deliver.
pub(crate) async fn compact_for_notification(
    compactor: &ContextCompactor<'_>,
    entity: &Value,
    compaction_cache: Option<&NotificationCompactionCache>,
) -> Result<Value, JsonLdError> {
    let compacted = match compact_entity_with_cache(compactor, entity, ContextStripping::Strip, compaction_cache).await {
        Ok(value) => value,
        Err(error) => {
            debug!(%error, "Failed to compact notification entity with subscription context; falling back to the core context");
            let core_only = ContextSpec::core_only();
            compact_entity_with_cache(&compactor.engine().compactor(&core_only), entity, ContextStripping::Strip, compaction_cache).await?
        }
    };
    Ok(normalize_compacted_entity(compacted))
}

/// Render a compacted entity in the subscription's `notification.format`
/// (clause 5.8.6): concise as clause 4.5.1 defines it, simplified (or its
/// synonym `keyValues`) as clause 4.5.4 does, and normalized as clause 4.5.1
/// and the per-family `.2` subclauses define it.
///
/// The normalized rendering is not the identity. A notification is compacted
/// from the stored expanded document, so a member with more than one written
/// spelling arrives in whichever one its Context Producer used: clause 4.5.22.2
/// makes a normalized `objectList` "an ordered array of Relationship Objects
/// each consisting of a JSON object containing a single Attribute with a key
/// called `object`", while clause 4.5.22.3 admits the bare URI string, and the
/// core `@context` gives `objectList` no `@type: "@id"` coercion that would
/// collapse the difference. `previousObjectList` takes the same shape, since
/// clause 4.5.22.2 makes its representation "the same as that of `objectList`".
///
/// A `join=inline` payload nests linked entities inside relationships, which
/// the typed [`Entity`] cannot carry, so that case is simplified on the JSON
/// value; every other case goes through the typed entity the way a query
/// response does. A value that cannot be rendered is delivered as it is.
pub(crate) fn render_format(value: Value, format: ResponseFormat, join: Option<JoinMode>) -> Value {
    match format {
        ResponseFormat::Normalized => {
            let mut normalized = value;
            normalize_object_lists(&mut normalized);
            normalized
        }
        ResponseFormat::Concise => concise_value(value),
        ResponseFormat::KeyValues | ResponseFormat::Simplified => {
            if join == Some(JoinMode::Inline) {
                match SnapshotRenderer::new(value.clone()).with_mode(ResponseFormat::Simplified).render() {
                    // clone: the renderer consumes its input and the value is the fallback
                    Ok(simplified) => simplified,
                    Err(error) => {
                        debug!(%error, "Failed to simplify inline-joined notification payload");
                        value
                    }
                }
            } else {
                match deserialize_exact::<Entity>(&value) {
                    Ok(entity) => simplify_entity(&entity),
                    Err(error) => {
                        debug!(%error, "Failed to simplify entity for keyValues");
                        value
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::representation::render_format;
    use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
    use serde_json::{Value, json};

    fn compacted_entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 20},
            "linkedTo": {"type": "Relationship", "object": "urn:ngsi-ld:Vehicle:2"}
        })
    }

    #[test]
    fn the_normalized_format_leaves_every_other_member_untouched() {
        pretty_assertions::assert_eq!(render_format(compacted_entity(), ResponseFormat::Normalized, None), compacted_entity());
    }

    /// Clause 4.5.22.2, Mandatory: a normalized `objectList` is "an ordered
    /// array of Relationship Objects each consisting of a JSON object
    /// containing a single Attribute with a key called `object` and its value
    /// holding the Relationship's object represented by a URI", and the same
    /// clause gives `previousObjectList` "the same as that of `objectList`".
    /// A producer that wrote the concise spelling of clause 4.5.22.3 does not
    /// decide the representation a normalized notification is delivered in.
    #[test]
    fn the_normalized_format_writes_the_object_keyed_members_of_a_list_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:3"],
                "previousObjectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
            }
        });
        let rendered = render_format(entity, ResponseFormat::Normalized, None);
        pretty_assertions::assert_eq!(rendered["hasRooms"]["objectList"], json!([{"object": "urn:ngsi-ld:Room:3"}]));
        pretty_assertions::assert_eq!(
            rendered["hasRooms"]["previousObjectList"],
            json!([{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}])
        );
        assert!(rendered["hasRooms"].get("object").is_none(), "clause 4.5.22.2 prohibits object: {rendered}");
        assert!(
            rendered["hasRooms"].get("previousObject").is_none(),
            "clause 4.5.22.2 prohibits previousObject: {rendered}"
        );
    }

    /// Table 5.2.37-1, concise form: "each string in the array holds a valid
    /// URI". Clause 4.5.22.2 gives `previousObjectList` the same representation
    /// as `objectList`, so a `showChanges` notification collapses both.
    #[test]
    fn the_concise_format_writes_the_bare_uris_of_a_list_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Building:2",
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": [{"object": "urn:ngsi-ld:Room:3"}],
                "previousObjectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]
            }
        });
        let rendered = render_format(entity, ResponseFormat::Concise, None);
        pretty_assertions::assert_eq!(rendered["hasRooms"]["objectList"], json!(["urn:ngsi-ld:Room:3"]));
        pretty_assertions::assert_eq!(rendered["hasRooms"]["previousObjectList"], json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]));
    }

    /// Clause 4.5.4, `ListRelationship` bullet: the simplified rendering is "an
    /// ordered array holding the `ListRelationship`'s Objects (represented as
    /// URIs)", whichever representation the normalized member was written in.
    #[test]
    fn the_simplified_format_writes_the_bare_uris_of_a_list_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Building:3",
            "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]}
        });
        let rendered = render_format(entity, ResponseFormat::KeyValues, None);
        pretty_assertions::assert_eq!(rendered["hasRooms"], json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]));
    }

    /// Clause 4.5.22.2: "An array consisting of a single NGSI-LD Null … can be
    /// used as the right-hand side of the `objectList` … in notifications …
    /// (for encoding a deleted `ListRelationship`)." The Null is not a
    /// Relationship Object, so the deletion keeps its single-element array.
    #[test]
    fn the_normalized_format_keeps_the_deletion_encoding_of_a_list_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Building:4",
            "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]}
        });
        let rendered = render_format(entity, ResponseFormat::Normalized, None);
        pretty_assertions::assert_eq!(rendered["hasRooms"]["objectList"], json!(["urn:ngsi-ld:null"]));
    }

    /// Clause 4.5.1: concise drops the explicit Property wrapper of a bare
    /// value and the `type` of a Relationship.
    #[test]
    fn the_concise_format_drops_the_explicit_property_wrapper() {
        let rendered = render_format(compacted_entity(), ResponseFormat::Concise, None);
        pretty_assertions::assert_eq!(rendered["speed"], json!(20));
        pretty_assertions::assert_eq!(rendered["linkedTo"], json!({"object": "urn:ngsi-ld:Vehicle:2"}));
    }

    /// Clause 4.5.4: `simplified` and its synonym `keyValues` render the bare
    /// value, with or without an inline join.
    #[test]
    fn the_simplified_formats_render_the_bare_value() {
        for format in [ResponseFormat::Simplified, ResponseFormat::KeyValues] {
            for join in [None, Some(JoinMode::Flat), Some(JoinMode::Inline)] {
                let rendered = render_format(compacted_entity(), format, join);
                pretty_assertions::assert_eq!(rendered["speed"], json!(20), "{format:?} under {join:?}");
                pretty_assertions::assert_eq!(rendered["linkedTo"], json!("urn:ngsi-ld:Vehicle:2"), "{format:?} under {join:?}");
            }
        }
    }
}
