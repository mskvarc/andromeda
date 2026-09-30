//! Expanding, once per request, the attribute member names a response carries.
//!
//! Matching a clause 4.21 projection term or a clause 5.7.2 `attrs` name
//! against an attribute member needs the member name's expanded IRI, and
//! expanding it needs the request `@context`, which only this layer holds.
//! [`AttributeExpansions`] expands each distinct name once per request and
//! answers every later lookup from its own index, so narrowing a page costs
//! one expansion per distinct name rather than one per member, per entity, per
//! pass. Whatever applies a projection further out — the renderer nesting
//! linked entities above all — receives the index already resolved instead of
//! a JSON-LD engine.
//!
//! [`AttributeExpansions`]: crate::query::attribute_expansion::AttributeExpansions

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, query::EntityQueryService};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{projection::attribute_iri_index::AttributeIriIndex, term_expander::TermExpander};
use iri_rs::IriBuf;

/// The expanded IRI of every attribute member name one request has met,
/// under the context that request prepared.
#[derive(Debug)]
pub struct AttributeExpansions<'a> {
    /// The engine bound to the request's prepared context.
    expander: ContextualExpander<'a>,
    /// Every name expanded so far and the IRI it expanded to.
    index: AttributeIriIndex,
}

impl<'a> AttributeExpansions<'a> {
    /// No name expanded yet, under the context `context` prepared.
    #[must_use]
    pub fn new(engine: &'a JsonLdEngine, context: &'a PreparedTermContext) -> AttributeExpansions<'a> {
        AttributeExpansions {
            expander: ContextualExpander::new(engine, context),
            index: AttributeIriIndex::new(),
        }
    }

    /// Expand `name` unless an earlier lookup already did.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the name cannot be expanded through the
    /// context.
    pub async fn ensure(&mut self, name: &AttributeName) -> Result<(), EntityError> {
        if self.index.expanded(name.as_str()).is_none() {
            let expanded = self.expander.expand_term(name.as_str()).await?;
            self.index.insert(name.clone(), expanded); // clone: the index owns the names it maps
        }
        Ok(())
    }

    /// Expand every Attribute member name of `entity`, the three clause 5.2.4
    /// geo-properties included, that no earlier lookup expanded.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when a member name cannot be expanded through
    /// the context.
    pub async fn cover(&mut self, entity: &Entity) -> Result<(), EntityError> {
        for name in entity.attribute_member_names() {
            if self.index.expanded(name).is_some() {
                continue;
            }
            // The geo-properties reach here as the fixed spellings the model
            // keys them by, so the name is validated once, on first sight.
            let key: AttributeName = name
                .parse()
                .map_err(|source| EntityError::Validation(EntityValidationReason::AttributeMemberName(source)))?;
            let expanded = self.expander.expand_term(name).await?;
            self.index.insert(key, expanded);
        }
        Ok(())
    }

    /// The IRI `name` expanded to, or `None` when no lookup expanded it.
    #[must_use]
    pub fn expanded(&self, name: &str) -> Option<&IriBuf> {
        self.index.expanded(name)
    }

    /// Every expansion made, as the index a renderer matches projections with.
    #[must_use]
    pub fn into_index(self) -> AttributeIriIndex {
        self.index
    }
}

impl EntityQueryService {
    /// The expanded IRI of every attribute key the given entities carry.
    ///
    /// The context is prepared once and each distinct key expanded once, so a
    /// key repeated across entities costs one lookup.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub async fn attribute_iri_index<'a>(
        &self,
        entities: impl IntoIterator<Item = &'a Entity>,
        context: &ContextSpec,
    ) -> Result<AttributeIriIndex, EntityError> {
        let prepared = self.engine().prepare_term_context(context);
        let mut expansions = AttributeExpansions::new(self.engine(), &prepared);
        for entity in entities {
            for key in entity.attrs.keys() {
                expansions.ensure(key).await?;
            }
        }
        Ok(expansions.into_index())
    }
}

#[cfg(test)]
mod tests {
    use crate::query::attribute_expansion::AttributeExpansions;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::entity::Entity;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    fn context() -> ContextSpec {
        ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }))
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("valid IRI")
    }

    #[tokio::test]
    async fn an_ensured_name_resolves_to_its_expansion_under_the_request_context() {
        let engine = engine();
        let prepared = engine.prepare_term_context(&context());
        let mut expansions = AttributeExpansions::new(&engine, &prepared);

        expansions.ensure(&name("speed")).await.expect("expands");

        pretty_assertions::assert_eq!(expansions.expanded("speed"), Some(&iri("https://example.com/speed")));
    }

    #[tokio::test]
    async fn a_name_no_lookup_expanded_resolves_to_nothing() {
        let engine = engine();
        let prepared = engine.prepare_term_context(&context());
        let expansions = AttributeExpansions::new(&engine, &prepared);

        assert!(expansions.expanded("speed").is_none());
    }

    /// Clause 5.2.4 keeps the geo-properties outside the attribute map, and a
    /// narrowing must still be able to match them by name.
    #[tokio::test]
    async fn covering_an_entity_expands_its_geo_properties_with_its_attributes() {
        let engine = engine();
        let prepared = engine.prepare_term_context(&context());
        let mut expansions = AttributeExpansions::new(&engine, &prepared);
        let entity: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }))
        .expect("entity");

        expansions.cover(&entity).await.expect("covers");

        pretty_assertions::assert_eq!(expansions.expanded("speed"), Some(&iri("https://example.com/speed")));
        pretty_assertions::assert_eq!(expansions.expanded("location"), Some(&iri("https://uri.etsi.org/ngsi-ld/location")));
    }

    #[tokio::test]
    async fn the_index_carries_every_expansion_made() {
        let engine = engine();
        let prepared = engine.prepare_term_context(&context());
        let mut expansions = AttributeExpansions::new(&engine, &prepared);

        expansions.ensure(&name("speed")).await.expect("expands");
        let index = expansions.into_index();

        pretty_assertions::assert_eq!(index.expanded("speed"), Some(&iri("https://example.com/speed")));
    }
}
