//! The part of an entity a Context Source answered a split temporal query
//! with, read as the Temporal Evolution it is (clause 5.7.4.4).
//!
//! A source's temporal answer is the same representation a client writes
//! (clauses 4.5.7 and 5.6.11), so it is read into the model as a written one
//! is, and the filters of the query read it through the entity traits exactly
//! as they read any other entity. The reified Scope (clause 4.5.6) is the
//! history of the paths the entity is in, not an attribute, so it is read as
//! the paths it dates.

use crate::{reified_scope::scope_observations, scope_observation::ScopeObservation};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::borrow::Cow;

/// One answered part of an entity, as the filters of a split query read it.
#[derive(Debug)]
pub(crate) struct AnsweredEvolution<'a> {
    /// The instances the answer carries.
    document: &'a EntityDocument,
    /// The paths the answer's reified Scope instances name (clause 4.5.6).
    scopes: Vec<ScopePath>,
}

impl<'a> AnsweredEvolution<'a> {
    /// The part `answer` is.
    pub(crate) fn of(answer: &'a TemporalDocument) -> AnsweredEvolution<'a> {
        let document = answer.entity();
        let scopes = document
            .attributes()
            .get(expanded_properties::SCOPE.as_str())
            .map(reified_paths)
            .unwrap_or_default();
        AnsweredEvolution { document, scopes }
    }
}

/// The paths every reified Scope instance of `scope` names; an instance that
/// ends the Scope names none.
fn reified_paths(scope: &AttributeMember) -> Vec<ScopePath> {
    scope
        .instances()
        .iter()
        .filter_map(|instance| scope_observations(instance.value()).ok())
        .flatten()
        .filter_map(|observation| match observation {
            ScopeObservation::Path(path) => Some(path),
            ScopeObservation::Deleted => None,
        })
        .collect()
}

impl<'a> QueriedEntity<'a> for &'a AnsweredEvolution<'a> {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        // The Scope is not an attribute a filter reads.
        let named_scope = attribute.as_str() == expanded_properties::SCOPE.as_str();
        self.document.attribute_instances(attribute).filter(move |_| !named_scope)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        QueriedEntity::types(self.document)
    }
}

impl<'a> LocatedEntity<'a> for &'a AnsweredEvolution<'a> {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.document.geo_property_geometries(attribute)
    }
}

impl<'a> ScopedEntity<'a> for &'a AnsweredEvolution<'a> {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        ScopedEntity::scopes(self.document).chain(self.scopes.iter())
    }
}

#[cfg(test)]
mod tests {
    use crate::answered_evolution::AnsweredEvolution;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_ngsi_ld_model::{
        received::ReceivedElement,
        temporal::{received_temporal::ReceivedTemporalEntities, temporal_body::TemporalBody, temporal_document::TemporalDocument},
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        queried_entity::{linked_entities::NoLinkedEntities, scoped_entity::ScopedEntity},
        scope::query::ScopeQuery,
        split_filter::{LeafTruths, SplitFilter},
    };
    use serde_json::{Value, json};

    fn context() -> ContextSpec {
        ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "speed": "https://example.com/speed",
                    "brand": "https://example.com/brand",
                    "says": "https://example.com/says",
                    "settings": "https://example.com/settings",
                    "gender": "https://example.com/gender",
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        )
    }

    fn selection(q: Option<&str>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q_expr(q.map(|raw| raw.parse().expect("a valid q")))
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("a valid scopeQ")))
            .build()
    }

    /// The temporal entity a source answered with, expanded under the context.
    async fn answered(answer: &Value) -> TemporalDocument {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
        let context = context();
        let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
        let ReceivedTemporalEntities(mut elements) = ReceivedTemporalEntities::from_slice(answer.to_string().as_bytes()).expect("an answer");
        let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
            panic!("the answer is a temporal entity");
        };
        let TemporalBody::Evolution(entity) = contextual.body else {
            panic!("the answer is a Temporal Evolution");
        };
        expansion.temporal(entity).await.expect("the answer expands")
    }

    /// The truths of the leaves of `selection` over `answer`.
    fn truths_of(selection: &EntitySelection, answer: &TemporalDocument) -> LeafTruths {
        SplitFilter::of(selection).truths_of(&AnsweredEvolution::of(answer), NoLinkedEntities)
    }

    /// Clause 4.5.7: the instances of an answered attribute are the ones the
    /// filter reads, each instance's value.
    #[tokio::test]
    async fn an_answer_is_read_as_instances() {
        let answer = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 40, "observedAt": "2020-08-01T11:00:00Z"},
                {"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"}
            ]
        }))
        .await;
        let above = selection(Some("\"https://example.com/speed\">50"), None);
        assert!(SplitFilter::of(&above).holds(&truths_of(&above, &answer)), "the second instance holds 60");
        let far_above = selection(Some("\"https://example.com/speed\">70"), None);
        assert!(!SplitFilter::of(&far_above).holds(&truths_of(&far_above, &answer)));
    }

    /// The families clause 4.5.9 wraps in a one-member object are read by the
    /// value they carry, so an existence filter holds over each.
    #[tokio::test]
    async fn every_family_is_read_by_the_value_it_carries() {
        for (term, attribute) in [
            (
                "says",
                json!({"type": "LanguageProperty", "languageMap": {"en": "yes"}, "observedAt": "2020-08-01T12:00:00Z"}),
            ),
            (
                "settings",
                json!({"type": "JsonProperty", "json": {"mode": "eco"}, "observedAt": "2020-08-01T12:00:00Z"}),
            ),
            (
                "gender",
                json!({"type": "VocabProperty", "vocab": "Male", "observedAt": "2020-08-01T12:00:00Z"}),
            ),
        ] {
            let answer = answered(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", term: [attribute]})).await;
            let exists = selection(Some(&format!("\"https://example.com/{term}\"")), None);
            assert!(SplitFilter::of(&exists).holds(&truths_of(&exists, &answer)), "{term}");
        }
    }

    /// Clauses 4.5.6 and 4.18: the temporal representation reifies the Scope,
    /// and the paths its instances name are the scopes a scope query reads.
    #[tokio::test]
    async fn a_reified_scope_answer_names_its_paths() {
        let answer = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/Madrid", "observedAt": "2020-08-01T12:00:00Z"}]
        }))
        .await;
        let madrid = selection(None, Some("/Madrid"));
        let part = AnsweredEvolution::of(&answer);
        let scopes: Vec<&ScopePath> = ScopedEntity::scopes(&part).collect();
        pretty_assertions::assert_eq!(scopes, vec![&"/Madrid".parse::<ScopePath>().expect("a scope path")]);
        assert!(SplitFilter::of(&madrid).holds(&truths_of(&madrid, &answer)));
    }
}
