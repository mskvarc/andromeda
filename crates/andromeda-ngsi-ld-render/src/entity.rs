//! The clause 4.5.1 rendering of a typed [`Entity`].
//!
//! [`EntityRenderer`] is the whole pipeline in one place — language
//! negotiation, `join=inline`, and the concise or simplified transformation —
//! so that no caller reaches for a stage of it directly and no caller can run
//! the stages in a different order. Each stage lives in its own module; this
//! one owns only the order they run in.

use crate::{
    concise::concise_value,
    error::{RenderedShape, RepresentationError},
    inline_join::apply_inline_join,
    language_negotiation::render_attribute_value,
    simplify::{simplify_entity, simplify_value},
};
use andromeda_ngsi_ld_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::rfc3339::whole_seconds;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Render an entity to a response value, applying the optional NGSI-LD `lang`
/// filter, `join=inline`, and concise/simplified transformation at the
/// presentation boundary.
///
/// The rendering pipeline executes in order:
/// 1. Serialize entity (with language filter if set)
/// 2. Apply inline join (if linked entities provided)
/// 3. Apply concise/simplified transformation (if mode != Normalized)
pub struct EntityRenderer<'a> {
    /// The resource being rendered.
    entity: &'a Entity,
    /// The `lang` filter to negotiate language properties against, if any.
    language_filter: Option<&'a LanguageFilter>,
    /// The entities `join=inline` resolves relationships against, if any.
    inline_linked_entities: Option<&'a HashMap<String, Entity>>,
    /// The clause 4.5.1 rendering to produce.
    mode: ResponseFormat,
}

impl<'a> EntityRenderer<'a> {
    /// Create a new renderer for the given entity in [`ResponseFormat::Normalized`] mode.
    pub const fn new(entity: &'a Entity) -> EntityRenderer<'a> {
        EntityRenderer {
            entity,
            language_filter: None,
            inline_linked_entities: None,
            mode: ResponseFormat::Normalized,
        }
    }

    /// Set the language filter for language property negotiation.
    pub const fn with_language_filter(mut self, filter: &'a LanguageFilter) -> Self {
        self.language_filter = Some(filter);
        self
    }

    /// Convenience: set the language filter from an `Option`, doing nothing if `None`.
    pub const fn with_optional_language_filter(mut self, filter: Option<&'a LanguageFilter>) -> Self {
        self.language_filter = filter;
        self
    }

    /// Set linked entities for `join=inline` expansion (NGSI-LD clause 4.5.23).
    pub const fn with_inline_linked_entities(mut self, linked: &'a HashMap<String, Entity>) -> Self {
        self.inline_linked_entities = Some(linked);
        self
    }

    /// Set the output representation mode.
    pub const fn with_mode(mut self, mode: ResponseFormat) -> Self {
        self.mode = mode;
        self
    }
}

impl<'a> EntityRenderer<'a> {
    /// Execute the full rendering pipeline and return the JSON value.
    ///
    /// Pipeline order:
    /// 1. Serialize entity (with language negotiation if filter set)
    /// 2. Apply inline join (if linked entities provided)
    /// 3. Apply concise/simplified transformation (if mode != Normalized)
    ///
    /// # Errors
    ///
    /// Returns [`RepresentationError::Serialization`] when the entity cannot be turned
    /// into a JSON value.
    pub fn render(self) -> Result<Value, RepresentationError> {
        // Fast path: simplified without language filter or inline join can use
        // the domain-level simplify() directly, avoiding a full serialize round-trip.
        if self.mode.is_simplified() && self.language_filter.is_none() && self.inline_linked_entities.is_none() {
            return Ok(simplify_entity(self.entity));
        }

        // Step 1: serialize (with optional language negotiation)
        let mut value = if self.language_filter.is_none() {
            serialize_value(self.entity)?
        } else {
            let mut map = Map::new();
            map.insert("id".to_string(), serialize_value(&self.entity.id)?);
            map.insert("type".to_string(), serialize_value(&self.entity.type_)?);

            if let Some(scope) = &self.entity.scope {
                map.insert("scope".to_string(), serialize_value(scope)?);
            }
            if let Some(expires_at) = &self.entity.expires_at {
                map.insert("expiresAt".to_string(), Value::String(whole_seconds(*expires_at)));
            }
            if let Some(location) = &self.entity.location {
                map.insert("location".to_string(), render_attribute_value(location, self.language_filter)?);
            }
            if let Some(observation_space) = &self.entity.observation_space {
                map.insert("observationSpace".to_string(), render_attribute_value(observation_space, self.language_filter)?);
            }
            if let Some(operation_space) = &self.entity.operation_space {
                map.insert("operationSpace".to_string(), render_attribute_value(operation_space, self.language_filter)?);
            }
            if let Some(created_at) = &self.entity.created_at {
                map.insert("createdAt".to_string(), Value::String(whole_seconds(*created_at)));
            }
            if let Some(modified_at) = &self.entity.modified_at {
                map.insert("modifiedAt".to_string(), Value::String(whole_seconds(*modified_at)));
            }
            if let Some(deleted_at) = &self.entity.deleted_at {
                map.insert("deletedAt".to_string(), Value::String(whole_seconds(*deleted_at)));
            }

            for (key, attr) in &self.entity.attrs {
                map.insert(key.to_string(), render_attribute_value(attr, self.language_filter)?);
            }

            Value::Object(map)
        };

        // Step 2: inline join
        if let Some(linked) = self.inline_linked_entities {
            apply_inline_join(&mut value, linked, self.language_filter)?;
        }

        // Step 3: concise/simplified transformation
        match self.mode {
            ResponseFormat::Normalized => {}
            ResponseFormat::Concise => {
                value = concise_value(value);
            }
            // Clause 4.5.4 gives `keyValues` and `simplified` one rendering.
            ResponseFormat::KeyValues | ResponseFormat::Simplified => {
                value = simplify_value(value);
            }
        }

        Ok(value)
    }
}

/// Serialize a value into its entity rendering.
///
/// Every rendering here ends in `serde_json`, and this is the one place the
/// failure is turned into a [`RepresentationError`] so the shape is not restated at
/// each call.
pub(crate) fn serialize_value<T>(value: &T) -> Result<Value, RepresentationError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| RepresentationError::serialization(RenderedShape::Entity, error))
}

#[cfg(test)]
mod tests {
    use crate::{entity::EntityRenderer, simplify::simplify_entity};
    use andromeda_ngsi_ld_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    #[test]
    fn rendering_without_language_filter_matches_entity_serialization() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:000",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Eiffel Tower"
            },
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));

        let rendered = EntityRenderer::new(&entity).render().expect("rendered entity");
        let serialized = serde_json::to_value(&entity).expect("serialized entity");

        pretty_assertions::assert_eq!(rendered, serialized);
    }

    #[test]
    fn simplifying_without_language_filter_matches_entity_simplification() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:004",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));

        let simplified = EntityRenderer::new(&entity)
            .with_mode(ResponseFormat::Simplified)
            .render()
            .expect("simplified entity");

        pretty_assertions::assert_eq!(simplified, simplify_entity(&entity));
    }

    #[test]
    fn language_filter_returns_exact_match_before_broader_match() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr-CA": "Rue Principale",
                    "fr": "Grand Place"
                }
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("language filter");

        let rendered = EntityRenderer::new(&entity).with_language_filter(&filter).render().expect("rendered entity");

        pretty_assertions::assert_eq!(rendered["street"]["type"], "Property");
        pretty_assertions::assert_eq!(rendered["street"]["value"], "Grand Place");
        pretty_assertions::assert_eq!(rendered["street"]["lang"], "fr");
    }

    #[test]
    fn language_filter_uses_wildcard_when_no_specific_match_exists() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:002",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));
        let filter = LanguageFilter::parse("*").expect("language filter");

        let rendered = EntityRenderer::new(&entity).with_language_filter(&filter).render().expect("rendered entity");

        pretty_assertions::assert_eq!(rendered["street"]["type"], "Property");
        pretty_assertions::assert_eq!(rendered["street"]["value"], "Grand Place");
        pretty_assertions::assert_eq!(rendered["street"]["lang"], "fr");
    }

    #[test]
    fn key_values_simplification_returns_scalar_after_language_negotiation() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:003",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("language filter");

        let simplified = EntityRenderer::new(&entity)
            .with_language_filter(&filter)
            .with_mode(ResponseFormat::Simplified)
            .render()
            .expect("rendered entity");

        pretty_assertions::assert_eq!(simplified["street"], "Grand Place");
    }

    #[test]
    fn render_mode_with_mode_method() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:011",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_mode(ResponseFormat::Simplified).render().expect("simplified");
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn with_optional_language_filter_none_has_no_effect() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:012",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_optional_language_filter(None).render().expect("rendered");
        let expected = serde_json::to_value(&entity).expect("serialized");
        pretty_assertions::assert_eq!(result, expected);
    }

    #[test]
    fn with_optional_language_filter_some_applies_filter() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:013",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("filter");
        let result = EntityRenderer::new(&entity)
            .with_optional_language_filter(Some(&filter))
            .render()
            .expect("rendered");
        pretty_assertions::assert_eq!(result["street"]["lang"], "fr");
    }

    #[test]
    fn renderer_preserves_collate_sub_attribute_on_property() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:collate",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Eiffel Tower",
                "collate": {
                    "type": "Property",
                    "value": "eiffel tower"
                }
            }
        }));

        let rendered = EntityRenderer::new(&entity).render().expect("rendered entity");
        pretty_assertions::assert_eq!(rendered["name"]["collate"]["value"], "eiffel tower");
        pretty_assertions::assert_eq!(rendered["name"]["value"], "Eiffel Tower");
    }

    #[test]
    fn renderer_preserves_collate_on_nested_sub_attributes() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:nested",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Eiffel Tower",
                "collate": {
                    "type": "Property",
                    "value": "eiffel tower"
                },
                "origin": {
                    "type": "Property",
                    "value": "Paris",
                    "collate": {
                        "type": "Property",
                        "value": "paris"
                    }
                }
            }
        }));

        let rendered = EntityRenderer::new(&entity).render().expect("rendered entity");
        pretty_assertions::assert_eq!(rendered["name"]["collate"]["value"], "eiffel tower");
        pretty_assertions::assert_eq!(rendered["name"]["origin"]["collate"]["value"], "paris");
        pretty_assertions::assert_eq!(rendered["name"]["origin"]["value"], "Paris");
    }

    #[test]
    fn renderer_preserves_collate_in_concise_mode() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:concise",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Tower",
                "collate": {
                    "type": "Property",
                    "value": "tower"
                }
            }
        }));

        let rendered = EntityRenderer::new(&entity).with_mode(ResponseFormat::Concise).render().expect("concise");
        pretty_assertions::assert_eq!(rendered["name"]["collate"]["value"], "tower");
    }
}
