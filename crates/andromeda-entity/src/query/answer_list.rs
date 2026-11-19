//! The list a query answers its merged Entities by (clauses 4.21, 5.7.2.4
//! and 5.7.2.5).
//!
//! A Context Source returns every Attribute it holds, and under split
//! entities so does the local read, so the request's `attrs`, `pick` or
//! `omit` is applied to the merged Entities. `attrs` is also a filter: an
//! Entity holding none of its Attributes does not match (clause 5.7.2.4). A
//! projection reducing an Entity to no member at all leaves it unanswered, by
//! the rule the local read selects by ([`ProjectedMembers`]).

use crate::query::{
    projection_remainder::remainder_under,
    state_projection::{keeps_a_standing_attribute, narrow_state},
};
use andromeda_linked_retrieval::narrowing::{list::ListInForce, list_terms::ListTerms};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, projection::projected_members::ProjectedMembers};
use chrono::{DateTime, Utc};

/// The clause 4.21 list a query answers its merged Entities by, with what an
/// Entity has to carry to be answered under it.
#[derive(Debug, Clone, Copy)]
pub struct AnswerList<'q> {
    /// The `attrs`, `pick` or `omit` each answered Entity is reduced by.
    list: ListInForce<'q>,
    /// What an Entity has to carry for the projection to leave it a member.
    members: &'q ProjectedMembers,
}

impl<'q> AnswerList<'q> {
    /// The list of a query that named no `attrs`, `pick` or `omit`: every
    /// Entity is answered whole.
    pub const UNRESTRICTED: AnswerList<'static> = AnswerList {
        list: ListInForce::Unrestricted,
        members: &ProjectedMembers::Unconditional,
    };

    /// The list `compiled` answers by.
    #[must_use]
    pub fn of(compiled: &'q CompiledEntityQuery) -> AnswerList<'q> {
        AnswerList {
            list: ListInForce::of(Some(&compiled.projection), &compiled.selection.attrs),
            members: &compiled.selection.projected_members,
        }
    }

    /// The `entities` the list answers at `now`, each reduced to it, in the
    /// order they come.
    ///
    /// An Entity is answered when it holds a standing Attribute the `attrs`
    /// list names, or, under a projection, when it keeps a member: an
    /// attribute whose every instance expired at `now` is not one the
    /// compacted Entity carries (clause 4.22).
    #[must_use]
    pub fn answered<T: AsRef<EntityState> + AsMut<EntityState>>(self, mut entities: Vec<T>, now: DateTime<Utc>) -> Vec<T> {
        if self.list.is_unrestricted() {
            return entities;
        }
        for entity in &mut entities {
            narrow_state(entity.as_mut(), self.list);
        }
        let listed = matches!(self.list, ListInForce::Payload(ListTerms::Attributes(_)));
        entities.retain(|entity| {
            let state = entity.as_ref();
            (!listed || keeps_a_standing_attribute(state, now)) && remainder_under(state, self.members, now).is_members()
        });
        entities
    }
}

#[cfg(test)]
mod tests {
    use crate::query::answer_list::AnswerList;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        projection::{expanded_projection::ExpandedProjection, expanded_projection_term::ExpandedProjectionTerm, projected_members::ProjectedMembers},
    };
    use chrono::Utc;
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

    fn attribute(value: &str) -> ExpandedProjectionTerm {
        ExpandedProjectionTerm::Attribute {
            iri: iri(value),
            linked: Vec::new(),
        }
    }

    /// The query compiled to `projection`, with the member rule it selects
    /// by.
    fn projecting(projection: ExpandedProjection) -> CompiledEntityQuery {
        CompiledEntityQuery::builder()
            .selection(EntitySelection::builder().projected_members(ProjectedMembers::of(&projection)).build())
            .projection(projection)
            .build()
    }

    /// The query listing `attrs`.
    fn listing(attrs: &[&str]) -> CompiledEntityQuery {
        CompiledEntityQuery::builder()
            .selection(EntitySelection::builder().attrs(attrs.iter().map(|name| iri(name)).collect()).build())
            .build()
    }

    fn answered(compiled: &CompiledEntityQuery) -> Vec<ReceivedState> {
        AnswerList::of(compiled).answered(merged(), Utc::now())
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
        let compiled = projecting(ExpandedProjection::builder().pick(vec![ExpandedProjectionTerm::Id, attribute(NAME)]).build());
        let narrowed = answered(&compiled);
        pretty_assertions::assert_eq!(names(&narrowed), vec![NAME]);
        assert!(narrowed[0].state.scope().is_none());
    }

    /// Clause 4.21: an `omit` removes the attribute it names and keeps the rest.
    #[test]
    fn an_omit_removes_its_attribute_from_a_merged_entity() {
        let compiled = projecting(ExpandedProjection::builder().omit(vec![attribute(LOCATION)]).build());
        let narrowed = answered(&compiled);
        pretty_assertions::assert_eq!(names(&narrowed), vec![NAME, SPEED]);
        assert!(narrowed[0].state.scope().is_some());
    }

    /// Clause 5.7.2.5: `attrs` reduces a merged entity to the attributes it
    /// names.
    #[test]
    fn attrs_narrow_a_merged_entity_to_the_attributes_they_name() {
        pretty_assertions::assert_eq!(names(&answered(&listing(&[SPEED]))), vec![SPEED]);
    }

    /// Clause 5.7.2.4: `attrs` naming nothing the entity holds leaves no answer.
    #[test]
    fn attrs_naming_nothing_held_leave_no_merged_entity() {
        pretty_assertions::assert_eq!(answered(&listing(&["https://example.com/other"])).len(), 0);
    }

    /// Clause 4.21: a `pick` keeping `id` answers the entity whatever else it
    /// holds.
    #[test]
    fn a_pick_keeping_the_id_keeps_the_bare_entity() {
        let compiled = projecting(ExpandedProjection::builder().pick(vec![ExpandedProjectionTerm::Id]).build());
        let narrowed = answered(&compiled);
        pretty_assertions::assert_eq!(narrowed.len(), 1);
        pretty_assertions::assert_eq!(names(&narrowed), Vec::<&str>::new());
    }

    /// Clause 5.7.2.5: a `pick` that would reduce the merged entity to no
    /// member at all leaves it unanswered, as the local read never selects
    /// such an entity.
    #[test]
    fn a_pick_leaving_no_member_leaves_no_merged_entity() {
        let compiled = projecting(ExpandedProjection::builder().pick(vec![attribute("https://example.com/other")]).build());
        pretty_assertions::assert_eq!(answered(&compiled).len(), 0);
    }

    /// No list in force narrows nothing.
    #[test]
    fn an_unrestricted_list_keeps_every_member() {
        let narrowed = AnswerList::UNRESTRICTED.answered(merged(), Utc::now());
        pretty_assertions::assert_eq!(names(&narrowed).len(), 3);
    }
}
