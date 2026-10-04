//! The part of an entity a Context Source answered a split temporal query
//! with, read as the Temporal Evolution it is (clause 5.7.4.4).
//!
//! A source's temporal answer is the same representation a client writes
//! (clauses 4.5.9 and 5.6.11), so it is read through the same steps a written
//! one is: the simplified temporal attributes unfolded on the Temporal
//! Property the query ran on, the reified Scope taken out and read
//! as the paths it dates, and the rest expanded as an entity document. The
//! filters of the query then read it through the entity traits, exactly as
//! they read any other entity.

use crate::{
    answer_narrowing::AnswerNarrowing,
    error::TemporalError,
    reified_scope::scope_observations,
    scope_observation::ScopeObservation,
    written_evolution::{EvolutionDocuments, read_evolution_document},
};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    term_expander::TermExpander,
};
use serde_json::Value;
use std::borrow::Cow;

/// One answered part of an entity, as the filters of a split query read it.
#[derive(Debug)]
pub(crate) struct AnsweredEvolution {
    /// The instances the answer carries, with any non-reified Scope.
    document: EntityDocument,
    /// The paths the answer's reified Scope instances name (clause 4.5.6).
    scopes: Vec<ScopePath>,
}

impl AnsweredEvolution {
    /// Read `answer` under `expansion`, narrowed by `narrowing` to the members
    /// the query's filters read, its `temporalValues` pairs made on
    /// `timeproperty`, the Temporal Property the query ran on.
    ///
    /// # Errors
    ///
    /// Returns the errors of reading a written Temporal Evolution when the
    /// answer, narrowed to the members the filters read, is none the model
    /// admits.
    pub(crate) async fn read<X: TermExpander>(
        expansion: &EntityExpansion<'_>,
        narrowing: &mut AnswerNarrowing<'_, X>,
        answer: &Value,
        timeproperty: TimeProp,
    ) -> Result<AnsweredEvolution, TemporalError> {
        let narrowed = narrowing.narrow(answer).await;
        let EvolutionDocuments { document, reified_scope } = read_evolution_document(expansion, narrowed, timeproperty).await?;
        let scopes = reified_scope.as_ref().map(reified_paths).unwrap_or_default();
        Ok(AnsweredEvolution { document, scopes })
    }
}

/// The paths every reified Scope instance of `scope` names; an instance that
/// ends the Scope names none.
fn reified_paths(scope: &EntityDocument) -> Vec<ScopePath> {
    scope
        .attributes()
        .values()
        .flat_map(AttributeMember::instances)
        .filter_map(|instance| scope_observations(instance.value()).ok())
        .flatten()
        .filter_map(|observation| match observation {
            ScopeObservation::Path(path) => Some(path),
            ScopeObservation::Deleted => None,
        })
        .collect()
}

impl<'a> QueriedEntity<'a> for &'a AnsweredEvolution {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        self.document.attribute_instances(attribute)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        QueriedEntity::types(&self.document)
    }
}

impl<'a> LocatedEntity<'a> for &'a AnsweredEvolution {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.document.geo_property_geometries(attribute)
    }
}

impl<'a> ScopedEntity<'a> for &'a AnsweredEvolution {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        ScopedEntity::scopes(&self.document).chain(self.scopes.iter())
    }
}

#[cfg(test)]
mod tests {
    use crate::{answer_narrowing::AnswerNarrowing, answered_evolution::AnsweredEvolution, error::TemporalError};
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_lexicon::{scope::path::ScopePath, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        queried_entity::{linked_entities::NoLinkedEntities, scoped_entity::ScopedEntity},
        scope::query::ScopeQuery,
        split_filter::{LeafTruths, SplitFilter},
    };
    use serde_json::{Value, json};

    fn context() -> ContextSpec {
        ContextSpec::from_body(json!({
            "speed": "https://example.com/speed",
            "brand": "https://example.com/brand",
            "says": "https://example.com/says",
            "settings": "https://example.com/settings",
            "gender": "https://example.com/gender",
        }))
    }

    fn selection(q: Option<&str>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q_expr(q.map(|raw| raw.parse().expect("a valid q")))
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("a valid scopeQ")))
            .build()
    }

    /// Read `answer` for the filters of `selection`, and the truths of their
    /// leaves over it.
    async fn read(selection: &EntitySelection, answer: &Value) -> Result<(AnsweredEvolution, LeafTruths), TemporalError> {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
        let context = context();
        let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
        let prepared = engine.prepare_term_context(&context);
        let expander = ContextualExpander::new(&engine, &prepared);
        let mut narrowing = AnswerNarrowing::new(selection, &expander);
        let part = AnsweredEvolution::read(&expansion, &mut narrowing, answer, TimeProp::Observed).await?;
        let truths = SplitFilter::of(selection).truths_of(&part, NoLinkedEntities);
        Ok((part, truths))
    }

    /// Clause 4.5.9: a `temporalValues` attribute holds its instances as
    /// `[value, instant]` pairs, and a filter reads each pair's value.
    #[tokio::test]
    async fn a_temporal_values_answer_is_read_as_instances() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "values": [[40, "2020-08-01T11:00:00Z"], [60, "2020-08-01T12:00:00Z"]]}
        });
        let above = selection(Some("\"https://example.com/speed\">50"), None);
        let (_, truths) = read(&above, &answer).await.expect("the answer reads");
        assert!(SplitFilter::of(&above).holds(&truths), "the second pair holds 60");
        let far_above = selection(Some("\"https://example.com/speed\">70"), None);
        let (_, truths) = read(&far_above, &answer).await.expect("the answer reads");
        assert!(!SplitFilter::of(&far_above).holds(&truths));
    }

    /// Clause 4.5.9 wraps the value of a `LanguageProperty`, `JsonProperty`
    /// and `VocabProperty` pair in a one-member object, which is the family's
    /// value member once, so each answer reads and an existence filter holds.
    #[tokio::test]
    async fn a_wrapped_temporal_values_answer_is_read_as_instances() {
        for (term, attribute) in [
            (
                "says",
                json!({"type": "LanguageProperty", "languageMaps": [[{"languageMap": {"en": "yes"}}, "2020-08-01T12:00:00Z"]]}),
            ),
            (
                "settings",
                json!({"type": "JsonProperty", "jsons": [[{"json": {"mode": "eco"}}, "2020-08-01T12:00:00Z"]]}),
            ),
            (
                "gender",
                json!({"type": "VocabProperty", "vocabs": [[{"vocab": "Male"}, "2020-08-01T12:00:00Z"]]}),
            ),
        ] {
            let answer = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", term: attribute});
            let exists = selection(Some(&format!("\"https://example.com/{term}\"")), None);
            let (_, truths) = read(&exists, &answer).await.unwrap_or_else(|error| panic!("{term}: {error}"));
            assert!(SplitFilter::of(&exists).holds(&truths), "{term}");
        }
    }

    /// Clauses 4.5.6 and 4.18: the temporal representation reifies the Scope,
    /// and the paths its instances name are the scopes a scope query reads.
    #[tokio::test]
    async fn a_reified_scope_answer_names_its_paths() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/Madrid", "observedAt": "2020-08-01T12:00:00Z"}]
        });
        let madrid = selection(None, Some("/Madrid"));
        let (part, truths) = read(&madrid, &answer).await.expect("the answer reads");
        let scopes: Vec<&ScopePath> = ScopedEntity::scopes(&part).collect();
        pretty_assertions::assert_eq!(scopes, vec![&"/Madrid".parse::<ScopePath>().expect("a scope path")]);
        assert!(SplitFilter::of(&madrid).holds(&truths));
    }

    /// Only the members the filters read are read: a member the write
    /// expansion refuses -- here an aggregate, which only a response carries
    /// -- cannot keep the readable ones from being judged.
    #[tokio::test]
    async fn members_the_filter_does_not_read_are_not_expanded() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"}],
            "brand": {"type": "Property", "avg": [[1, "2020-08-01T00:00:00Z", "2020-08-02T00:00:00Z"]]}
        });
        let speed = selection(Some("\"https://example.com/speed\">50"), None);
        let (_, truths) = read(&speed, &answer).await.expect("the speed is read alone");
        assert!(SplitFilter::of(&speed).holds(&truths));
        let brand = selection(Some("\"https://example.com/brand\"==1"), None);
        assert!(read(&brand, &answer).await.is_err(), "a filter reading the aggregate cannot read it");
    }
}
