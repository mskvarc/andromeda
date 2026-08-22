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
    engine::{options::ContextStripping, processor::JsonLdEngine},
    error::JsonLdError,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_render::{concise::concise_value, simplify::simplify_entity, snapshot::SnapshotRenderer};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde_json::Value;
use tracing::debug;

/// Compact an expanded entity under the subscription's context, falling back
/// to the core context when that context cannot be applied.
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
    engine: &JsonLdEngine,
    entity: &Value,
    context: &ContextSpec,
    compaction_cache: Option<&NotificationCompactionCache>,
) -> Result<Value, JsonLdError> {
    let compacted = match compact_entity_with_cache(engine, entity, context, ContextStripping::Strip, compaction_cache).await {
        Ok(value) => value,
        Err(error) => {
            debug!(%error, "Failed to compact notification entity with subscription context; falling back to the core context");
            compact_entity_with_cache(engine, entity, &ContextSpec::core_only(), ContextStripping::Strip, compaction_cache).await?
        }
    };
    Ok(normalize_compacted_entity(compacted))
}

/// Render a compacted entity in the subscription's `notification.format`
/// (clause 5.8.6): concise as clause 4.5.1 defines it, simplified (or its
/// synonym `keyValues`) as clause 4.5.4 does, and normalized as it is.
///
/// A `join=inline` payload nests linked entities inside relationships, which
/// the typed [`Entity`] cannot carry, so that case is simplified on the JSON
/// value; every other case goes through the typed entity the way a query
/// response does. A value that cannot be rendered is delivered as it is.
pub(crate) fn render_format(value: Value, format: ResponseFormat, join: Option<JoinMode>) -> Value {
    match format {
        ResponseFormat::Normalized => value,
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
    fn the_normalized_format_is_the_identity() {
        pretty_assertions::assert_eq!(render_format(compacted_entity(), ResponseFormat::Normalized, None), compacted_entity());
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
