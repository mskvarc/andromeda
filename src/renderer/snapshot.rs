//! Renderer for pre-compacted `serde_json::Value` payloads.
//!
//! While [`crate::renderer::entity::EntityRenderer`] operates on an `&Entity`
//! domain object, [`SnapshotRenderer`] operates on a `Value` that has already
//! gone through JSON-LD compaction. It serves two use cases:
//!
//! 1. Snapshot responses (ETSI GS CIM 009 clause 5.5.15), where the entity
//!    data is frozen in JSONB storage and no `Entity` struct is reconstructed.
//! 2. Notification payload assembly, where the compacted `Value` must receive
//!    an inline-join expansion and/or concise/simplified transformation before
//!    being delivered.

use crate::{
    core::error::AndromedaError,
    domain::entity::Entity,
    renderer::{
        Renderer,
        entity::{RenderMode, apply_inline_join, concise_value, simplify_value},
    },
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Renders a pre-compacted NGSI-LD JSON value with optional attribute
/// filtering, inline-join expansion, and representation-mode transformation.
#[derive(Debug)]
pub struct SnapshotRenderer<'a> {
    payload: Value,
    attrs_filter: Option<&'a [&'a str]>,
    mode: RenderMode,
    inline_linked_entities: Option<&'a HashMap<String, Entity>>,
}

impl<'a> SnapshotRenderer<'a> {
    /// Create a renderer for the given compacted payload.
    pub const fn new(payload: Value) -> SnapshotRenderer<'a> {
        SnapshotRenderer {
            payload,
            attrs_filter: None,
            mode: RenderMode::Normalized,
            inline_linked_entities: None,
        }
    }

    /// Restrict the output to the given attribute keys (plus `id`, `type`, `@context`).
    pub const fn with_attrs_filter(mut self, attrs: Option<&'a [&'a str]>) -> Self {
        self.attrs_filter = attrs;
        self
    }

    /// Set the output representation mode.
    pub const fn with_mode(mut self, mode: RenderMode) -> Self {
        self.mode = mode;
        self
    }

    /// Provide linked entities for `join=inline` expansion.
    pub const fn with_inline_linked_entities(mut self, linked: &'a HashMap<String, Entity>) -> Self {
        self.inline_linked_entities = Some(linked);
        self
    }
}

impl<'a> Renderer for SnapshotRenderer<'a> {
    type Output = Value;

    fn render(self) -> Result<Value, AndromedaError> {
        let SnapshotRenderer {
            mut payload,
            attrs_filter,
            mode,
            inline_linked_entities,
        } = self;

        if let Some(requested) = attrs_filter
            && let Some(obj) = payload.as_object_mut()
        {
            let keys_to_keep: HashSet<&str> = ["id", "type", "@context"].iter().copied().chain(requested.iter().copied()).collect();
            obj.retain(|k, _| keys_to_keep.iter().any(|keep| k.ends_with(keep)));
        }

        if let Some(linked) = inline_linked_entities {
            apply_inline_join(&mut payload, linked, None)?;
        }

        let transformed = match mode {
            RenderMode::Normalized => payload,
            RenderMode::Concise => concise_value(payload),
            RenderMode::Simplified => simplify_value(payload),
        };

        Ok(transformed)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn render(payload: Value, attrs: Option<&[&str]>, mode: RenderMode) -> Value {
        SnapshotRenderer::new(payload)
            .with_attrs_filter(attrs)
            .with_mode(mode)
            .render()
            .expect("render")
    }

    #[test]
    fn preserves_collate_on_normalized() {
        let value = json!({
            "id": "urn:x", "type": "T",
            "name": {
                "type": "Property",
                "value": "Tower",
                "collate": {"type": "Property", "value": "tower"}
            }
        });
        let result = render(value, None, RenderMode::Normalized);
        pretty_assertions::assert_eq!(result["name"]["collate"]["value"], "tower");
        pretty_assertions::assert_eq!(result["name"]["value"], "Tower");
    }

    #[test]
    fn filters_attrs() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "x"}, "temp": {"type": "Property", "value": 25}});
        let result = render(value, Some(&["name"]), RenderMode::Normalized);
        assert!(result.get("name").is_some());
        assert!(result.get("temp").is_none());
        assert!(result.get("id").is_some());
        assert!(result.get("type").is_some());
    }

    #[test]
    fn applies_simplified_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value, None, RenderMode::Simplified);
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn applies_concise_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value, None, RenderMode::Concise);
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn normalized_mode_is_identity() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value.clone(), None, RenderMode::Normalized);
        pretty_assertions::assert_eq!(result, value);
    }

    #[test]
    fn filters_then_transforms() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}, "temp": {"type": "Property", "value": 25}});
        let result = render(value, Some(&["name"]), RenderMode::Simplified);
        pretty_assertions::assert_eq!(result["name"], "Tower");
        assert!(result.get("temp").is_none());
    }

    #[test]
    fn inline_join_expands_relationship() {
        let linked_entity: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Room:1",
            "type": "Room",
            "name": {"type": "Property", "value": "Conference Room"}
        }))
        .expect("entity");
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:ngsi-ld:Room:1".to_string(), linked_entity);

        let entity = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}
        });

        let result = SnapshotRenderer::new(entity)
            .with_inline_linked_entities(&linked_map)
            .render()
            .expect("inline join");
        assert!(result["hasRoom"]["entity"].is_object());
        pretty_assertions::assert_eq!(result["hasRoom"]["entity"]["id"], "urn:ngsi-ld:Room:1");
    }
}
