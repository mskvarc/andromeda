//! Renderer for pre-compacted `serde_json::Value` payloads.
//!
//! While [`crate::entity::EntityRenderer`] operates on an `&Entity`
//! domain object, [`SnapshotRenderer`] operates on a `Value` that has already
//! gone through JSON-LD compaction. It serves two use cases:
//!
//! 1. Snapshot responses (ETSI GS CIM 009 clause 5.5.15), where the entity
//!    data is frozen in JSONB storage and no `Entity` struct is reconstructed.
//! 2. Notification payload assembly, where the compacted `Value` must receive
//!    an inline-join expansion and/or concise/simplified transformation before
//!    being delivered.

use crate::{
    concise::concise_value,
    error::RenderError,
    inline_join::apply_inline_join,
    simplify::simplify_value,
    structural_projection::prune_structural_members,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use indexmap::IndexMap;
use serde_json::Value;

/// Renders a pre-compacted NGSI-LD JSON value with optional attribute
/// filtering, inline-join expansion, and representation-mode transformation.
#[derive(Debug)]
pub struct SnapshotRenderer<'a> {
    /// The already-compacted payload to render.
    payload: Value,
    /// The attribute keys the response is narrowed to, if it is narrowed.
    attrs_filter: Option<&'a [AttributeName]>,
    /// The clause 4.5.1 rendering to produce.
    mode: ResponseFormat,
    /// The entities `join=inline` resolves relationships against, if any.
    inline_linked_entities: Option<&'a IndexMap<Uri, Entity>>,
    /// The clause 6.3.15 `pick`/`omit` structural-member rule to apply to
    /// the payload's `id`, `type` and `scope` keys, if any.
    structural_projection: Option<&'a Projection>,
}

impl<'a> SnapshotRenderer<'a> {
    /// Create a renderer for the given compacted payload.
    pub const fn new(payload: Value) -> SnapshotRenderer<'a> {
        SnapshotRenderer {
            payload,
            attrs_filter: None,
            mode: ResponseFormat::Normalized,
            inline_linked_entities: None,
            structural_projection: None,
        }
    }

    /// Restrict the output to the given attribute keys (plus `id`, `type`, `@context`).
    pub const fn with_attrs_filter(mut self, attrs: Option<&'a [AttributeName]>) -> Self {
        self.attrs_filter = attrs;
        self
    }

    /// Set the output representation mode.
    pub const fn with_mode(mut self, mode: ResponseFormat) -> Self {
        self.mode = mode;
        self
    }

    /// Provide linked entities for `join=inline` expansion.
    pub const fn with_inline_linked_entities(mut self, linked: &'a IndexMap<Uri, Entity>) -> Self {
        self.inline_linked_entities = Some(linked);
        self
    }

    /// Apply the clause 6.3.15 structural-member rule to the payload's `id`,
    /// `type` and `scope` keys.
    pub const fn with_structural_projection(mut self, projection: &'a Projection) -> Self {
        self.structural_projection = Some(projection);
        self
    }

    /// Apply the attribute filter, the inline join and the representation mode.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when a linked entity brought in
    /// by `join=inline` cannot be turned into a JSON value.
    pub fn render(self) -> Result<Value, RenderError> {
        let SnapshotRenderer {
            mut payload,
            attrs_filter,
            mode,
            inline_linked_entities,
            structural_projection,
        } = self;

        if let Some(requested) = attrs_filter
            && let Some(obj) = payload.as_object_mut()
        {
            // `attrs` is a synonym for `pick`: the entity is reduced to the
            // named members plus the mandatory id/type (and the JSON-LD
            // `@context`). Match keys exactly -- a suffix match kept `surname`
            // for `attrs=name` and kept any key ending in `id`/`type`.
            obj.retain(|key, _| matches!(key.as_str(), "id" | "type" | "@context") || requested.iter().any(|name| name.as_str() == key));
        }

        if let Some(linked) = inline_linked_entities {
            apply_inline_join(&mut payload, linked, None, None)?;
        }

        let mut transformed = match mode {
            ResponseFormat::Normalized => payload,
            ResponseFormat::Concise => concise_value(payload),
            // Clause 4.5.4 gives `keyValues` and `simplified` one rendering.
            ResponseFormat::KeyValues | ResponseFormat::Simplified => simplify_value(payload),
        };

        if let Some(projection) = structural_projection {
            prune_structural_members(&mut transformed, projection);
        }

        Ok(transformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
    use serde_json::json;

    fn render(payload: Value, attrs: Option<&[AttributeName]>, mode: ResponseFormat) -> Value {
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
        let result = render(value, None, ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(result["name"]["collate"]["value"], "tower");
        pretty_assertions::assert_eq!(result["name"]["value"], "Tower");
    }

    #[test]
    fn filters_attrs() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "x"}, "temp": {"type": "Property", "value": 25}});
        let attrs = ["name".parse().expect("attribute name")];
        let result = render(value, Some(&attrs), ResponseFormat::Normalized);
        assert!(result.get("name").is_some());
        assert!(result.get("temp").is_none());
        assert!(result.get("id").is_some());
        assert!(result.get("type").is_some());
    }

    #[test]
    fn attrs_filter_matches_exact_keys_not_suffixes() {
        // `attrs` is a synonym for `pick` (spec Table 6.3.x): the entity is
        // reduced to the named members plus the mandatory id/type. A suffix
        // match kept `surname` for `attrs=name` and kept any key ending in
        // `id`/`type`; only the exact key survives. `scope` is a filterable
        // member (attrs=pick reduces to the list), so it is dropped when the
        // list does not name it.
        let value = json!({
            "id": "urn:x", "type": "T",
            "name": {"type": "Property", "value": "Ada"},
            "surname": {"type": "Property", "value": "Lovelace"},
            "scope": "/Madrid"
        });
        let attrs = ["name".parse().expect("attribute name")];
        let result = render(value, Some(&attrs), ResponseFormat::Normalized);
        assert!(result.get("name").is_some());
        assert!(result.get("surname").is_none(), "a suffix match must not keep surname for attrs=name");
        assert!(result.get("id").is_some());
        assert!(result.get("type").is_some());
        assert!(
            result.get("scope").is_none(),
            "scope is a filterable member and is dropped when attrs does not name it"
        );
    }

    #[test]
    fn applies_simplified_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value, None, ResponseFormat::Simplified);
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn applies_concise_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value, None, ResponseFormat::Concise);
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn normalized_mode_is_identity() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render(value.clone(), None, ResponseFormat::Normalized);
        pretty_assertions::assert_eq!(result, value);
    }

    #[test]
    fn filters_then_transforms() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}, "temp": {"type": "Property", "value": 25}});
        let attrs = ["name".parse().expect("attribute name")];
        let result = render(value, Some(&attrs), ResponseFormat::Simplified);
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
        let mut linked_map = IndexMap::new();
        linked_map.insert("urn:ngsi-ld:Room:1".parse().expect("valid URI"), linked_entity);

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

    /// Clause 6.3.15, literal reading: a non-empty `pick` naming only `id`
    /// keeps `id` and drops `type`.
    #[test]
    fn structural_projection_of_pick_id_leaves_only_id() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        let result = SnapshotRenderer::new(value).with_structural_projection(&projection).render().expect("render");

        assert_eq!(result["id"], "urn:x");
        assert!(result.get("type").is_none());
    }

    /// Clause 6.3.15: `omit` removes the structural members it lists, here
    /// `scope`, while `id` and `type` survive untouched.
    #[test]
    fn structural_projection_of_omit_scope_removes_scope() {
        let value = json!({"id": "urn:x", "type": "T", "scope": "/Madrid"});
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };

        let result = SnapshotRenderer::new(value).with_structural_projection(&projection).render().expect("render");

        assert_eq!(result["id"], "urn:x");
        assert_eq!(result["type"], "T");
        assert!(result.get("scope").is_none());
    }

    /// Clause 6.3.15: `omit` removes `type` when the omit list names it.
    #[test]
    fn structural_projection_of_omit_type_removes_type() {
        let value = json!({"id": "urn:x", "type": "T"});
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Type],
        };

        let result = SnapshotRenderer::new(value).with_structural_projection(&projection).render().expect("render");

        assert_eq!(result["id"], "urn:x");
        assert!(result.get("type").is_none());
    }

    /// `@context` is a JSON-LD transport concern, never a structural member,
    /// and survives the structural-member rule.
    #[test]
    fn structural_projection_never_touches_context() {
        let value = json!({"id": "urn:x", "type": "T", "@context": "https://example.com/context.jsonld"});
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        let result = SnapshotRenderer::new(value).with_structural_projection(&projection).render().expect("render");

        assert_eq!(result["@context"], "https://example.com/context.jsonld");
    }
}
