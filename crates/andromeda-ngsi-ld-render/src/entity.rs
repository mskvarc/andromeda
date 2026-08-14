//! The clause 4.5.1 rendering of a typed [`Entity`].
//!
//! [`EntityRenderer`] is the whole pipeline in one place — language
//! negotiation, `join=inline`, and the concise or simplified transformation —
//! so that no caller reaches for a stage of it directly and no caller can run
//! the stages in a different order. Each stage lives in its own module; this
//! one owns only the order they run in.

use crate::{
    concise::concise_value,
    error::{RenderError, RenderedShape},
    inline_join::apply_inline_join,
    language_negotiation::render_attribute_value,
    nested_projection::LinkedProjection,
    simplify::{simplify_entity, simplify_value},
    structural_projection::prune_structural_members,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{rfc3339::whole_seconds, uri::Uri};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

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
    inline_linked_entities: Option<&'a IndexMap<Uri, Entity>>,
    /// The clause 4.5.1 rendering to produce.
    mode: ResponseFormat,
    /// The clause 4.21 `pick`/`omit` structural-member rule to apply to
    /// the rendered value's `id`, `type` and `scope` keys, if any.
    structural_projection: Option<&'a Projection>,
    /// The clause 4.21 nested sub-projections `join=inline` applies to the
    /// linked entities it nests, if any.
    linked_projection: Option<LinkedProjection<'a>>,
}

impl<'a> EntityRenderer<'a> {
    /// Create a new renderer for the given entity in [`ResponseFormat::Normalized`] mode.
    pub const fn new(entity: &'a Entity) -> EntityRenderer<'a> {
        EntityRenderer {
            entity,
            language_filter: None,
            inline_linked_entities: None,
            mode: ResponseFormat::Normalized,
            structural_projection: None,
            linked_projection: None,
        }
    }

    /// Set the language filter for language property negotiation.
    ///
    /// Takes an `Option` so a caller that may or may not have negotiated a
    /// language has one spelling to reach for; `None` leaves the filter unset.
    pub const fn with_optional_language_filter(mut self, filter: Option<&'a LanguageFilter>) -> Self {
        self.language_filter = filter;
        self
    }

    /// Set linked entities for `join=inline` expansion (NGSI-LD clause 4.5.23).
    pub const fn with_inline_linked_entities(mut self, linked: &'a IndexMap<Uri, Entity>) -> Self {
        self.inline_linked_entities = Some(linked);
        self
    }

    /// Set the output representation mode.
    pub const fn with_mode(mut self, mode: ResponseFormat) -> Self {
        self.mode = mode;
        self
    }

    /// Apply the clause 4.21 structural-member rule to the rendered
    /// value's `id`, `type` and `scope` keys.
    ///
    /// Takes an `Option` so a caller that may or may not have compiled a
    /// projection has one spelling to reach for; `None` prunes nothing.
    pub const fn with_optional_structural_projection(mut self, projection: Option<&'a Projection>) -> Self {
        self.structural_projection = projection;
        self
    }

    /// Set the clause 4.21 nested sub-projections `join=inline` applies to the
    /// linked entities it nests.
    ///
    /// Takes an `Option` so a caller that may or may not have a projection has
    /// one spelling to reach for; `None` narrows nothing.
    pub const fn with_optional_linked_projection(mut self, projection: Option<LinkedProjection<'a>>) -> Self {
        self.linked_projection = projection;
        self
    }
}

/// Render a sequence of entities to the flat clause-4.5.23 collection, the
/// matching entities ahead of the entities the traversal reached.
///
/// `join=flat` returns the root entities and their resolved linked entities side
/// by side in one array — unlike `join=inline`, which nests the linked entities
/// inside each root. Both read paths (single-entity retrieval and query) share
/// this, so the flat shape is assembled in one place rather than hand-rolled at
/// each endpoint. A response with no join passes an empty `linked`.
///
/// The two groups are taken separately because the clause 4.21
/// structural-member rule applies only to the roots: `root_projection` names
/// what the *request* selected, whereas a linked entity is narrowed — if at
/// all — by the clause 4.21 sub-projection of the Relationship that reached
/// it, which the traversal has already applied by the time it gets here.
///
/// # Errors
///
/// Returns [`RenderError::Serialization`] when any entity cannot be
/// turned into a JSON value.
pub fn render_flat_join<'a>(
    roots: impl IntoIterator<Item = &'a Entity>,
    linked: impl IntoIterator<Item = &'a Entity>,
    language_filter: Option<&'a LanguageFilter>,
    mode: ResponseFormat,
    root_projection: Option<&'a Projection>,
) -> Result<Vec<Value>, RenderError> {
    let rendered_roots = roots.into_iter().map(|entity| {
        EntityRenderer::new(entity)
            .with_optional_language_filter(language_filter)
            .with_mode(mode)
            .with_optional_structural_projection(root_projection)
            .render()
    });
    let rendered_linked = linked.into_iter().map(|entity| {
        EntityRenderer::new(entity)
            .with_optional_language_filter(language_filter)
            .with_mode(mode)
            .render()
    });
    rendered_roots.chain(rendered_linked).collect()
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
    /// Returns [`RenderError::Serialization`] when the entity cannot be turned
    /// into a JSON value.
    pub fn render(self) -> Result<Value, RenderError> {
        // Fast path: simplified without language filter, inline join or a
        // structural projection can use the domain-level simplify()
        // directly, avoiding a full serialize round-trip.
        if self.mode.is_simplified() && self.language_filter.is_none() && self.inline_linked_entities.is_none() && self.structural_projection.is_none() {
            return Ok(simplify_entity(self.entity));
        }

        // Step 1: serialize the whole entity, then -- when a language filter is
        // set -- re-render only the members the filter touches. The
        // serialization already carries every entity member (id, type, scope,
        // the system timestamps, and anything the model later gains), so a new
        // member cannot be silently dropped here the way a hand-listed rebuild
        // would drop it. The rebuild that this replaces was the drop hazard: it
        // enumerated the members it knew and nothing else.
        let mut value = serialize_value(self.entity)?;
        if self.language_filter.is_some()
            && let Value::Object(map) = &mut value
        {
            // The four clause-4.8 timestamps keep the whole-second spelling the
            // rest of the render crate uses; unifying timestamp spellings across
            // paths is a separate concern and is not decided here.
            if let Some(expires_at) = &self.entity.expires_at {
                map.insert("expiresAt".to_string(), Value::String(whole_seconds(*expires_at)));
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
            // The geo-properties and every other attribute are re-rendered so a
            // language map inside them is negotiated; a non-language attribute
            // renders identically to its serialization.
            if let Some(location) = &self.entity.location {
                map.insert("location".to_string(), render_attribute_value(location, self.language_filter)?);
            }
            if let Some(observation_space) = &self.entity.observation_space {
                map.insert("observationSpace".to_string(), render_attribute_value(observation_space, self.language_filter)?);
            }
            if let Some(operation_space) = &self.entity.operation_space {
                map.insert("operationSpace".to_string(), render_attribute_value(operation_space, self.language_filter)?);
            }
            for (key, attr) in &self.entity.attrs {
                map.insert(key.to_string(), render_attribute_value(attr, self.language_filter)?);
            }
        }

        // Step 2: inline join
        if let Some(linked) = self.inline_linked_entities {
            apply_inline_join(&mut value, linked, self.language_filter, self.linked_projection)?;
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

        // Step 4: clause 4.21 structural-member pruning
        if let Some(projection) = self.structural_projection {
            prune_structural_members(&mut value, projection);
        }

        Ok(value)
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
    use crate::{
        entity::{EntityRenderer, render_flat_join},
        simplify::simplify_entity,
    };
    use andromeda_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::iter::once;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    #[test]
    fn render_flat_join_renders_the_roots_ahead_of_the_linked_entities() {
        // Clause 4.5.23 `join=flat`: the roots come first, then the entities
        // the traversal reached, each rendered independently.
        let root = parse_entity(json!({"id": "urn:ngsi-ld:A:1", "type": "A"}));
        let mut linked = IndexMap::new();
        linked.insert("urn:ngsi-ld:B:1".to_string(), parse_entity(json!({"id": "urn:ngsi-ld:B:1", "type": "B"})));
        let rendered = render_flat_join(once(&root), linked.values(), None, ResponseFormat::Normalized, None).expect("renders");
        pretty_assertions::assert_eq!(rendered.len(), 2);
        pretty_assertions::assert_eq!(rendered[0]["id"], "urn:ngsi-ld:A:1");
        pretty_assertions::assert_eq!(rendered[1]["id"], "urn:ngsi-ld:B:1");
    }

    /// Clause 4.21 narrows what the *request* selected, so the structural
    /// rule reaches the matching entities only. A linked entity carries
    /// whatever the clause 4.21 sub-projection of its Relationship left it
    /// with, and keeps its `id` so the flat collection stays navigable.
    #[test]
    fn render_flat_join_applies_the_structural_rule_to_the_roots_only() {
        let root = parse_entity(json!({"id": "urn:ngsi-ld:A:2", "type": "A"}));
        let linked = parse_entity(json!({"id": "urn:ngsi-ld:B:2", "type": "B"}));
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        };

        let rendered = render_flat_join(once(&root), once(&linked), None, ResponseFormat::Normalized, Some(&projection)).expect("renders");

        assert!(!rendered[0].as_object().expect("an entity object").contains_key("id"));
        pretty_assertions::assert_eq!(rendered[1]["id"], "urn:ngsi-ld:B:2");
        pretty_assertions::assert_eq!(rendered[1]["type"], "B");
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
        // The clause-4.8 timestamps keep the whole-second spelling.
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
            .with_optional_structural_projection(Some(&projection))
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
            .with_optional_structural_projection(Some(&projection))
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
            .with_optional_structural_projection(Some(&projection))
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
            .with_optional_structural_projection(Some(&projection))
            .render()
            .expect("rendered entity");
        let object = rendered.as_object().expect("an entity object");

        assert!(!object.contains_key("id"));
        assert!(!object.contains_key("type"));
        pretty_assertions::assert_eq!(rendered["speed"]["value"], 10);
    }
}
