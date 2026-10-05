//! The clause 4.5 rendering of one typed [`Entity`] as a JSON document.
//!
//! [`EntityRenderer`] is for the callers that need the rendered entity as a
//! document to transform further -- a notification shaped as a document, a
//! re-assembled notification of a forwarded subscription, an entity nested
//! into a document by an inline join. It writes the entity with the very walk a
//! response body is written by ([`crate::entity_body`]), the clause 4.15
//! negotiation, the clause 4.21 structural rule and the clause 4.3.6.8 version
//! fallback included, and leaves every
//! Relationship target as the store holds it: whoever finishes the document
//! decodes them once, at the presentation boundary ([`crate::uri_decoding`]).

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    error::{RenderError, RenderedShape},
    structural_projection::StructuralProjection,
    uri_decoding::UriDecoding,
    version::fallback::VersionFallback,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::Serialize;
use serde_json::Value;

/// Render an entity to a JSON document, applying the optional NGSI-LD `lang`
/// filter, the clause 4.5 representation and the clause 4.21 structural rule.
pub struct EntityRenderer<'a> {
    /// The resource being rendered.
    entity: &'a Entity,
    /// The `lang` filter to negotiate language properties against, if any.
    language_filter: Option<&'a LanguageFilter>,
    /// The clause 4.5.1 rendering to produce.
    mode: ResponseFormat,
    /// The clause 4.21 `pick`/`omit` structural-member rule to apply to
    /// the rendered value's `id`, `type` and `scope` keys, if any.
    structural_projection: Option<StructuralProjection<'a>>,
    /// The clause 4.3.6.8 fallback the entity is written under, if any.
    fallback: Option<VersionFallback>,
}

impl<'a> EntityRenderer<'a> {
    /// Create a new renderer for the given entity in [`ResponseFormat::Normalized`] mode.
    #[must_use]
    pub const fn new(entity: &'a Entity) -> EntityRenderer<'a> {
        EntityRenderer {
            entity,
            language_filter: None,
            mode: ResponseFormat::Normalized,
            structural_projection: None,
            fallback: None,
        }
    }

    /// Set the language filter for language property negotiation.
    ///
    /// Takes an `Option` so a caller that may or may not have negotiated a
    /// language has one spelling to reach for; `None` leaves the filter unset.
    #[must_use]
    pub const fn with_optional_language_filter(mut self, filter: Option<&'a LanguageFilter>) -> Self {
        self.language_filter = filter;
        self
    }

    /// Set the output representation mode.
    #[must_use]
    pub const fn with_mode(mut self, mode: ResponseFormat) -> Self {
        self.mode = mode;
        self
    }

    /// Apply the clause 4.21 structural-member rule to the rendered
    /// value's `id`, `type` and `scope` keys.
    ///
    /// Takes an `Option` so a caller that may or may not have compiled a
    /// projection has one spelling to reach for; `None` prunes nothing.
    #[must_use]
    pub const fn with_optional_structural_projection(mut self, projection: Option<StructuralProjection<'a>>) -> Self {
        self.structural_projection = projection;
        self
    }

    /// Write the entity for an older NGSI-LD version (clause 4.3.6.8).
    ///
    /// Takes an `Option` so a caller that may or may not have a version to
    /// conform to has one spelling to reach for; `None` writes the entity as
    /// this broker's own version does.
    #[must_use]
    pub const fn with_optional_fallback(mut self, fallback: Option<VersionFallback>) -> Self {
        self.fallback = fallback;
        self
    }
}

impl EntityRenderer<'_> {
    /// Render the entity as a JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the entity cannot be turned
    /// into a JSON value.
    pub fn render(self) -> Result<Value, RenderError> {
        serialize_value(&TypedEntity {
            entity: self.entity,
            format: self.mode,
            walk: MemberWalk::new(UriDecoding::Keep, self.language_filter, self.fallback),
            projection: self.structural_projection,
            identifier: IdentifierMember::Written,
            context: None,
            stamped: None,
            join: None,
            narrowing: None,
            changes: None,
        })
    }
}

/// Serialize a value into its entity rendering.
///
/// Every rendering here ends in `serde_json`, and this is the one place the
/// failure is turned into a [`RenderError`] so the shape is not restated at
/// each call.
pub(crate) fn serialize_value<T>(value: &T) -> Result<Value, RenderError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
}

#[cfg(test)]
mod tests {
    use crate::{entity::EntityRenderer, simplify::simplify_entity, structural_projection::StructuralProjection};
    use andromeda_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

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

        pretty_assertions::assert_eq!(simplified, simplify_entity(&entity).expect("the entity simplifies"));
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

        let rendered = EntityRenderer::new(&entity)
            .with_optional_language_filter(Some(&filter))
            .render()
            .expect("rendered entity");

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

        let rendered = EntityRenderer::new(&entity)
            .with_optional_language_filter(Some(&filter))
            .render()
            .expect("rendered entity");

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
            .with_optional_language_filter(Some(&filter))
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
    fn language_filtering_preserves_every_system_member_alongside_the_negotiated_property() {
        // The filtered rendering serializes the whole entity and re-renders only
        // the members a language filter touches, so every system member the
        // entity carries -- scope and the clause-4.8 timestamps -- survives the
        // filter, and a member the model later gains survives too. A hand-listed
        // member set is the silent-drop hazard this guards against.
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:lang-members",
            "type": "Building",
            "scope": "/Madrid/Centro",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-02T00:00:00Z",
            "deletedAt": "2025-01-03T00:00:00Z",
            "expiresAt": "2030-01-01T00:00:00Z",
            "street": { "type": "LanguageProperty", "languageMap": { "fr": "Grand Place", "nl": "Grote Markt" } }
        }));
        let filter = LanguageFilter::parse("fr").expect("language filter");

        let rendered = EntityRenderer::new(&entity)
            .with_optional_language_filter(Some(&filter))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        for member in ["id", "type", "scope", "createdAt", "modifiedAt", "deletedAt", "expiresAt"] {
            assert!(object.contains_key(member), "the language filter dropped the {member} member");
        }
        // The language property was still negotiated down to the requested tag.
        pretty_assertions::assert_eq!(rendered["street"]["type"], "Property");
        pretty_assertions::assert_eq!(rendered["street"]["value"], "Grand Place");
        pretty_assertions::assert_eq!(rendered["street"]["lang"], "fr");
        // A whole-second clause-4.8 timestamp renders as a bare Z, no fraction.
        pretty_assertions::assert_eq!(rendered["createdAt"], "2025-01-01T00:00:00Z");
    }

    /// Clause 4.21, literal reading: a non-empty `pick` naming only `id`
    /// keeps `id` and drops `type` (and `scope`, when present). Ordinary
    /// attributes are outside this rule -- `pick`'s attribute half is
    /// applied earlier, on the typed `Entity`, by
    /// `EntityQueryService::apply_projection`.
    #[test]
    fn structural_projection_of_pick_id_drops_type_but_leaves_attributes_alone() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:pick-id",
            "type": "Building",
            "speed": {"type": "Property", "value": 10}
        }));
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        let rendered = EntityRenderer::new(&entity)
            .with_optional_structural_projection(Some(StructuralProjection::from(&projection)))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        assert_eq!(rendered["id"], "urn:ngsi-ld:Building:pick-id");
        assert!(!object.contains_key("type"));
        pretty_assertions::assert_eq!(rendered["speed"]["value"], 10);
    }

    /// Clause 4.21: `omit` removes the structural members it lists; here
    /// `scope` is dropped while `id` and `type` survive untouched.
    #[test]
    fn structural_projection_of_omit_scope_removes_scope() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:omit-scope",
            "type": "Building",
            "scope": "/Madrid"
        }));
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };

        let rendered = EntityRenderer::new(&entity)
            .with_optional_structural_projection(Some(StructuralProjection::from(&projection)))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        assert_eq!(rendered["id"], "urn:ngsi-ld:Building:omit-scope");
        assert_eq!(rendered["type"], "Building");
        assert!(!object.contains_key("scope"));
    }

    /// Clause 4.21: `omit` removes `type` when the omit list names it.
    #[test]
    fn structural_projection_of_omit_type_removes_type() {
        let entity = parse_entity(json!({"id": "urn:ngsi-ld:Building:omit-type", "type": "Building"}));
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Type],
        };

        let rendered = EntityRenderer::new(&entity)
            .with_optional_structural_projection(Some(StructuralProjection::from(&projection)))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        assert_eq!(rendered["id"], "urn:ngsi-ld:Building:omit-type");
        assert!(!object.contains_key("type"));
    }

    /// Clause 4.21, literal reading: a non-empty `pick` that names only an
    /// attribute leaves `id` and `type` out of the rendering, since neither
    /// keyword was listed.
    #[test]
    fn structural_projection_of_a_pick_attribute_leaves_id_and_type_out() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:pick-attr",
            "type": "Building",
            "speed": {"type": "Property", "value": 10}
        }));
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        };

        let rendered = EntityRenderer::new(&entity)
            .with_optional_structural_projection(Some(StructuralProjection::from(&projection)))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        assert!(!object.contains_key("id"));
        assert!(!object.contains_key("type"));
        pretty_assertions::assert_eq!(rendered["speed"]["value"], 10);
    }
}
