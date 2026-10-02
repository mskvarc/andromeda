//! Narrowing a stored entity state to the members a read selected, by
//! expanded name, before it is compacted.
//!
//! The stored state is keyed by expanded IRIs, and the clause 5.7.2 `attrs`
//! list and the clause 4.21 `pick`/`omit` terms are expanded when the request
//! is compiled, so both sides of every comparison are already IRIs: nothing a
//! response would carry has to be compacted first and expanded back to be
//! matched, and an attribute the selection drops is never compacted at all.
//! Compaction maps every IRI to a term that expands back to it, so selecting
//! by IRI before compaction keeps exactly the attributes selecting by
//! compacted name after it kept.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;

/// Retain only the attributes whose expanded name `wanted` lists (clause
/// 5.7.2 `attrs`). `scope` and the entity `expiresAt` are not attributes and
/// stay as they are.
pub(crate) fn retain_wanted_attributes(state: &mut EntityState, wanted: &[IriBuf]) {
    state.attributes_mut().retain(|name, _| wanted.iter().any(|iri| iri.as_str() == name.as_str()));
}

/// Apply a clause 4.21 projection to a state's attributes and its `scope`.
///
/// `id` and `type` are written by the renderer, which applies their half of
/// the keyword rule.
pub(crate) fn project_state(state: &mut EntityState, projection: &Projection) {
    if !projection.pick.is_empty() {
        state
            .attributes_mut()
            .retain(|name, _| projection.pick.iter().any(|term| names_attribute(term, name)));
    }
    if !projection.omit.is_empty() {
        // Clause 4.21: `omit=rel{a}` omits `a` inside the entity `rel` links
        // to, not `rel` itself, so only a term carrying no linked
        // sub-projection removes the attribute here.
        state
            .attributes_mut()
            .retain(|name, _| !projection.omit.iter().any(|term| names_attribute(term, name) && term.linked_terms().is_empty()));
    }
    if !projection.keeps(&ProjectionTerm::Scope) {
        state.set_scope(None);
    }
}

/// Whether any attribute of `state` still has an instance standing at `now`
/// (clause 4.22): the attributes the compacted entity will carry.
pub(crate) fn keeps_a_standing_attribute(state: &EntityState, now: DateTime<Utc>) -> bool {
    state.attributes().names().any(|name| state.attribute_stands(name.as_str(), now))
}

/// Whether `term` names the attribute stored under the expanded `name`.
///
/// The same match as [`ProjectionTerm::matches_attribute`] with the key and
/// its expansion both the stored IRI.
fn names_attribute(term: &ProjectionTerm, name: &AttributeName) -> bool {
    match term {
        ProjectionTerm::Attribute { iri, .. } => iri.as_str() == name.as_str(),
        ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::query::state_projection::{keeps_a_standing_attribute, project_state, retain_wanted_attributes};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    const SPEED: &str = "https://example.com/speed";
    const NAME: &str = "https://example.com/name";
    const OWNER: &str = "https://example.com/owner";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn property(value: &Value) -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    fn state() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            SPEED: property(&json!(80)),
            NAME: property(&json!("Bus")),
            OWNER: [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:1"}]}],
            LOCATION: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1.0}, {"@value": 2.0}]}]
                }]
            }]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state")
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("valid IRI")
    }

    fn attribute(value: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(value), linked }
    }

    fn names(state: &EntityState) -> Vec<&str> {
        state.attributes().names().map(AttributeName::as_str).collect()
    }

    /// Clause 5.7.2: `attrs` keeps the listed attributes, the clause 5.2.4
    /// geo-properties among them, and leaves `scope` alone.
    #[test]
    fn wanted_attributes_keep_only_the_listed_names_and_the_scope() {
        let mut state = state();
        retain_wanted_attributes(&mut state, &[iri(SPEED), iri(LOCATION)]);
        pretty_assertions::assert_eq!(names(&state), vec![SPEED, LOCATION]);
        assert!(state.scope().is_some());
    }

    /// Clause 4.21: a `pick` naming attributes keeps them and, not naming the
    /// `scope` keyword, drops `scope`.
    #[test]
    fn a_pick_keeps_its_attributes_and_drops_an_unnamed_scope() {
        let mut state = state();
        let projection = Projection {
            pick: vec![attribute(NAME, Vec::new()), attribute(OWNER, vec![attribute(NAME, Vec::new())])],
            omit: Vec::new(),
        };
        project_state(&mut state, &projection);
        pretty_assertions::assert_eq!(names(&state), vec![NAME, OWNER]);
        assert!(state.scope().is_none());
    }

    /// Clause 4.21: a bare `omit` removes the attribute, an `omit` carrying a
    /// linked sub-projection does not, and `scope` survives an `omit` that
    /// does not name it.
    #[test]
    fn an_omit_removes_only_its_bare_terms() {
        let mut state = state();
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![attribute(SPEED, Vec::new()), attribute(OWNER, vec![attribute(NAME, Vec::new())])],
        };
        project_state(&mut state, &projection);
        pretty_assertions::assert_eq!(names(&state), vec![NAME, OWNER, LOCATION]);
        assert!(state.scope().is_some());
    }

    /// Clause 4.21: the `scope` keyword is a member `omit` may remove and
    /// `pick` may keep.
    #[test]
    fn the_scope_keyword_decides_the_scope() {
        let mut omitted = state();
        project_state(
            &mut omitted,
            &Projection {
                pick: Vec::new(),
                omit: vec![ProjectionTerm::Scope],
            },
        );
        assert!(omitted.scope().is_none());
        pretty_assertions::assert_eq!(names(&omitted).len(), 4);

        let mut picked = state();
        project_state(
            &mut picked,
            &Projection {
                pick: vec![ProjectionTerm::Scope],
                omit: Vec::new(),
            },
        );
        assert!(picked.scope().is_some());
        pretty_assertions::assert_eq!(names(&picked), Vec::<&str>::new());
    }

    /// An empty projection narrows nothing.
    #[test]
    fn an_empty_projection_keeps_everything() {
        let mut state = state();
        project_state(&mut state, &Projection::default());
        pretty_assertions::assert_eq!(names(&state).len(), 4);
        assert!(state.scope().is_some());
    }

    /// Clause 4.22: an attribute whose every instance has expired is not one
    /// the compacted entity carries.
    #[test]
    fn only_an_unexpired_instance_keeps_an_attribute_standing() {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            SPEED: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}]
            }]
        }]);
        let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state");
        let before: DateTime<Utc> = "2029-12-31T00:00:00Z".parse().expect("instant");
        let after: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().expect("instant");
        assert!(keeps_a_standing_attribute(&state, before));
        assert!(!keeps_a_standing_attribute(&state, after));
    }
}
