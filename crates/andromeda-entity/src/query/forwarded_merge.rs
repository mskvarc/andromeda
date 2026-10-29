//! Merging locally held entities with those a context source returned
//! (clause 5.7.2).

use crate::query::{EntityQueryService, state_projection::narrow_state};
use andromeda_federation::{entity_result::merge_query_results, outcome::ForwardedEntities};
use andromeda_linked_retrieval::narrowing::{list::ListInForce, list_terms::ListTerms};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;

impl EntityQueryService {
    /// Merge local query results with forwarded context source results.
    ///
    /// The merge runs on the model, by expanded name, and keeps every member a
    /// holder contributed; [`EntityQueryService::narrow_merged`] applies the
    /// request's list to the result.
    #[must_use]
    pub fn merge_with_forwarded(local_entities: Vec<ReceivedState>, forwarded: Vec<ForwardedEntities>) -> Vec<ReceivedState> {
        merge_query_results(local_entities, forwarded)
    }

    /// Narrow merged entities to the request's list in force.
    ///
    /// Clauses 5.7.2 and 4.21 apply `attrs`, `pick` and `omit` after the merge,
    /// because a context source returns every attribute it holds. The list is
    /// matched against the expanded names. An entity left with no attribute by
    /// `attrs` is no answer (clause 5.7.2.4), while one left bare by `pick` or
    /// `omit` is still the entity that matched.
    #[must_use]
    pub fn narrow_merged(mut merged: Vec<ReceivedState>, list: ListInForce<'_>) -> Vec<ReceivedState> {
        if list.is_unrestricted() {
            return merged;
        }
        for version in &mut merged {
            narrow_state(&mut version.state, list);
        }
        if matches!(list, ListInForce::Payload(ListTerms::Attributes(_))) {
            merged.retain(|version| !version.state.attributes().is_empty());
        }
        merged
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::query::EntityQueryService;
    use andromeda_linked_retrieval::narrowing::list::ListInForce;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    const SPEED: &str = "https://example.com/speed";
    const NAME: &str = "https://example.com/name";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn property(value: &Value) -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    fn merged() -> Vec<ReceivedState> {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            SPEED: property(&json!(80)),
            NAME: property(&json!("Bus")),
            LOCATION: property(&json!("here"))
        }]);
        let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state");
        vec![ReceivedState {
            state,
            system_times: EntitySystemTimes::default(),
            linked: Vec::new(),
        }]
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("valid IRI")
    }

    fn attribute(value: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(value),
            linked: Vec::new(),
        }
    }

    fn names(versions: &[ReceivedState]) -> Vec<&str> {
        versions
            .iter()
            .flat_map(|version| version.state.attributes().names().map(AttributeName::as_str))
            .collect()
    }

    /// Clause 4.21: a `pick` keeps the picked attributes of a merged entity and
    /// drops the `scope` it does not name.
    #[test]
    fn a_pick_narrows_a_merged_entity_to_its_attributes() {
        let projection = Projection {
            pick: vec![ProjectionTerm::Id, attribute(NAME)],
            omit: Vec::new(),
        };
        let narrowed = EntityQueryService::narrow_merged(merged(), ListInForce::of(Some(&projection), &[]));
        pretty_assertions::assert_eq!(names(&narrowed), vec![NAME]);
        assert!(narrowed[0].state.scope().is_none());
    }

    /// Clause 4.21: an `omit` removes the attribute it names and keeps the rest.
    #[test]
    fn an_omit_removes_its_attribute_from_a_merged_entity() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![attribute(LOCATION)],
        };
        let narrowed = EntityQueryService::narrow_merged(merged(), ListInForce::of(Some(&projection), &[]));
        pretty_assertions::assert_eq!(names(&narrowed), vec![NAME, SPEED]);
        assert!(narrowed[0].state.scope().is_some());
    }

    /// Clause 5.7.2.4: `attrs` naming nothing the entity holds leaves no answer.
    #[test]
    fn attrs_naming_nothing_held_leave_no_merged_entity() {
        let wanted = [iri("https://example.com/other")];
        let narrowed = EntityQueryService::narrow_merged(merged(), ListInForce::of(None, &wanted));
        assert!(narrowed.is_empty());
    }

    /// Clause 4.21: a `pick` naming nothing the entity holds still answers with
    /// the entity.
    #[test]
    fn a_pick_naming_nothing_held_keeps_the_bare_entity() {
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };
        let narrowed = EntityQueryService::narrow_merged(merged(), ListInForce::of(Some(&projection), &[]));
        pretty_assertions::assert_eq!(narrowed.len(), 1);
        assert!(names(&narrowed).is_empty());
    }

    /// No list in force narrows nothing.
    #[test]
    fn an_unrestricted_list_keeps_every_member() {
        let narrowed = EntityQueryService::narrow_merged(merged(), ListInForce::Unrestricted);
        pretty_assertions::assert_eq!(names(&narrowed).len(), 3);
    }
}
