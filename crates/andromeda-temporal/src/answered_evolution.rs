//! The part of an entity a Context Source answered a split temporal query
//! with, read as the Temporal Evolution it is (clause 5.7.4.4).
//!
//! A source's temporal answer is the same representation a client writes
//! (clauses 4.5.7 and 5.6.11), so it is read into the model as a written one
//! is, and the filters of the query read it through the entity traits exactly
//! as they read any other entity. The reified Scope (clause 4.5.6) is the
//! history of the paths the entity is in, not an attribute, so it is read as
//! the paths it dates.
//!
//! A leaf reads the instances lying in the query's window, as the local leaf
//! does: an instance carrying no instant on the window's Temporal Property is
//! represented by no read on it (clause 4.5.7), so it satisfies no leaf even
//! when a source answers with it, and the Scope event or instance in effect
//! when the window opens is the latest across every part, which the merged
//! in-effect entity decides rather than any one part. A Scope a source states
//! with no instant on the window's axis counts: this broker dates such a Scope
//! at the write that states it (clause 4.18 makes it valid from the time it
//! has been set), and the instant a source set it at is not in its answer.

use crate::reified_scope::reified_paths;
use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    temporal::window::TemporalWindow,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::borrow::Cow;

/// One answered part of an entity, as the filters of a split query read it.
#[derive(Debug)]
pub(crate) struct AnsweredEvolution<'a> {
    /// The instances the answer carries.
    document: &'a EntityDocument,
    /// The paths the answer's reified Scope instances inside the window, or
    /// with no instant on its axis, name (clause 4.5.6).
    scopes: Vec<ScopePath>,
    /// The window whose attribute instances the leaves read.
    window: TemporalWindow,
}

impl<'a> AnsweredEvolution<'a> {
    /// The part `answer` is, read over `window`.
    pub(crate) fn of(answer: &'a TemporalDocument, window: TemporalWindow) -> AnsweredEvolution<'a> {
        let document = answer.entity();
        let scopes = document
            .attributes()
            .get(expanded_properties::SCOPE.as_str())
            .map_or_default(|scope| reified_paths(scope.instances().iter().filter(|instance| counts_in(instance, window))));
        AnsweredEvolution { document, scopes, window }
    }
}

/// Whether a reified Scope instance counts in `window`: it lies in it, or it
/// carries no instant on the window's axis and so states the Scope undated.
fn counts_in(instance: &InstanceDocument, window: TemporalWindow) -> bool {
    instance.lies_within(window) || instance.instant_on(window.timeproperty).is_none()
}

impl<'a> QueriedEntity<'a> for &'a AnsweredEvolution<'a> {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        // The Scope is not an attribute a filter reads.
        let named_scope = attribute.as_str() == expanded_properties::SCOPE.as_str();
        let window = self.window;
        self.document
            .attribute_instances(attribute)
            .filter(move |instance| !named_scope && instance.lies_within(window))
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        QueriedEntity::types(self.document)
    }
}

impl<'a> LocatedEntity<'a> for &'a AnsweredEvolution<'a> {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.attribute_instances(attribute)
            .filter_map(|instance| instance.geometry().map(Cow::Borrowed))
    }
}

impl<'a> ScopedEntity<'a> for &'a AnsweredEvolution<'a> {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        ScopedEntity::scopes(self.document).chain(self.scopes.iter())
    }
}

#[cfg(test)]
mod tests {
    use crate::{answered_evolution::AnsweredEvolution, answered_fixture::answered};
    use andromeda_ngsi_ld_lexicon::{scope::path::ScopePath, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        queried_entity::{linked_entities::NoLinkedEntities, scoped_entity::ScopedEntity},
        scope::query::ScopeQuery,
        split_filter::{LeafTruths, SplitFilter},
        temporal::window::{TemporalWindow, TimeInterval},
    };
    use serde_json::json;

    fn selection(q: Option<&str>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q_expr(q.map(|raw| raw.parse().expect("a valid q")))
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("a valid scopeQ")))
            .build()
    }

    /// A window on `observedAt` over `interval`.
    fn observed(interval: TimeInterval) -> TemporalWindow {
        TemporalWindow::builder().timeproperty(TimeProp::Observed).interval(interval).build()
    }

    /// The truths of the leaves of `selection` over `answer`, read over
    /// `window`.
    fn truths_over(selection: &EntitySelection, answer: &TemporalDocument, window: TemporalWindow) -> LeafTruths {
        SplitFilter::of(selection).truths_of(&AnsweredEvolution::of(answer, window), NoLinkedEntities)
    }

    /// The truths of the leaves of `selection` over `answer`, read over an
    /// unbounded window.
    fn truths_of(selection: &EntitySelection, answer: &TemporalDocument) -> LeafTruths {
        truths_over(selection, answer, observed(TimeInterval::Unbounded))
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
        let part = AnsweredEvolution::of(&answer, observed(TimeInterval::Unbounded));
        let scopes: Vec<&ScopePath> = ScopedEntity::scopes(&part).collect();
        pretty_assertions::assert_eq!(scopes, vec![&"/Madrid".parse::<ScopePath>().expect("a scope path")]);
        assert!(SplitFilter::of(&madrid).holds(&truths_of(&madrid, &answer)));
    }

    /// Clause 4.5.7: an instance carrying no `observedAt` is represented by no
    /// read on `observedAt`, and one outside the window is no instance of it,
    /// so neither satisfies a leaf of a query on that window, as neither does
    /// locally.
    #[tokio::test]
    async fn undated_remote_instances_do_not_satisfy_a_window_leaf() {
        let answer = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 60},
                {"type": "Property", "value": 70, "observedAt": "2020-08-01T11:00:00Z"},
                {"type": "Property", "value": 40, "observedAt": "2020-08-01T12:30:00Z"}
            ]
        }))
        .await;
        let above = selection(Some("\"https://example.com/speed\">50"), None);
        let noon = "2020-08-01T12:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        for interval in [TimeInterval::Unbounded, TimeInterval::After(noon)] {
            let held = truths_over(&above, &answer, observed(interval));
            pretty_assertions::assert_eq!(SplitFilter::of(&above).holds(&held), interval == TimeInterval::Unbounded, "{interval:?}");
        }
        let undated_only = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60}]
        }))
        .await;
        assert!(!SplitFilter::of(&above).holds(&truths_of(&above, &undated_only)));
    }

    /// Clause 4.18: a Scope a source states with no instant is valid from a
    /// time its answer does not carry, as this broker dates such a Scope at
    /// its write, so it counts in the window; a dated one before the window
    /// does not, the merged in-effect entity deciding it.
    #[tokio::test]
    async fn an_undated_scope_a_source_states_counts_in_the_window() {
        let barcelona = selection(None, Some("/Barcelona"));
        let noon = "2020-08-01T12:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let undated = answered(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "scope": "/Barcelona"})).await;
        assert!(SplitFilter::of(&barcelona).holds(&truths_over(&barcelona, &undated, observed(TimeInterval::After(noon)))));
        let before = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/Barcelona", "observedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        assert!(!SplitFilter::of(&barcelona).holds(&truths_over(&barcelona, &before, observed(TimeInterval::After(noon)))));
    }
}
