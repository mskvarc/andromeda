//! What a clause 4.21 projection leaves of an entity, and the operation's
//! answer when it leaves nothing.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.1.4 states the rule for the selection
//! spelled `attrs`: "If the optional Attribute list is present and the NGSI-LD
//! endpoint does know about a matching Entity for the Entity ID, but this
//! Entity does not have any of the Attributes in the Attribute list, then an
//! error of type `ResourceNotFound` shall be raised." Tables 6.4.3.2-1 and
//! 5.2.14.1-1 make `attrs` "a synonym for pick, except that id, type, scope
//! are not allowed", so a `pick` that names no member the entity has is the
//! very same failure; and clause 5.7.1.5 reduces the Entity "down to only
//! contain the defined Entity members", so an `omit` naming every member it
//! has leaves nothing to answer with either.
//!
//! A query answers the same question as a selection instead: an Entity the
//! projection would reduce to nothing is not selected at all
//! (`andromeda_ngsi_ld_query::projection::projected_members`), so the count and
//! the page are taken over the entities that keep a member.

use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use derive_more::IsVariant;

/// What a clause 4.21 projection left of an entity.
#[derive(Clone, Copy, Debug, Eq, IsVariant, PartialEq)]
pub enum ProjectionRemainder {
    /// At least one Entity member survived, so there is an entity to render.
    Members,
    /// Every Entity member was projected away, so there is nothing to render.
    Nothing,
}

/// What `projection` left of `entity`.
///
/// `entity` must already have been narrowed by
/// [`project_entity`]: this reads the members that survived rather than
/// reapplying the projection to find out.
///
/// [`project_entity`]: crate::query::attribute_projection::project_entity
#[must_use]
pub fn projection_remainder(entity: &Entity, projection: &Projection) -> ProjectionRemainder {
    if projection.is_empty() {
        return ProjectionRemainder::Members;
    }
    // `scope` is already gone if the projection excluded it; `id` and `type`
    // are members of the rendered representation rather than of the typed
    // entity, so the keyword rule is what decides whether they will be written.
    let survives = entity.has_attribute_members() || entity.scope.is_some() || projection.keeps(&ProjectionTerm::Id) || projection.keeps(&ProjectionTerm::Type);
    if survives {
        ProjectionRemainder::Members
    } else {
        ProjectionRemainder::Nothing
    }
}

#[cfg(test)]
mod tests {
    use crate::query::projection_remainder::{ProjectionRemainder, projection_remainder};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn attribute(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(&format!("https://example.com/{name}")),
            linked: Vec::new(),
        }
    }

    fn entity(payload: serde_json::Value) -> Entity {
        serde_json::from_value(payload).expect("entity")
    }

    fn stripped() -> Entity {
        entity(json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"}))
    }

    /// Clause 5.7.1.4: a `pick` that matched no member of the entity leaves
    /// nothing, because the clause 4.21 keyword rule drops `id` and `type`
    /// along with it.
    #[test]
    fn a_pick_naming_only_an_unmatched_attribute_leaves_nothing() {
        let projection = Projection {
            pick: vec![attribute("unknown")],
            omit: Vec::new(),
        };

        assert_eq!(projection_remainder(&stripped(), &projection), ProjectionRemainder::Nothing);
    }

    /// A `pick` that names `id` leaves the entity identifiable, so there is
    /// something to render even with no attribute left.
    #[test]
    fn a_pick_naming_id_leaves_the_identifier() {
        let projection = Projection {
            pick: vec![ProjectionTerm::Id],
            omit: Vec::new(),
        };

        assert_eq!(projection_remainder(&stripped(), &projection), ProjectionRemainder::Members);
    }

    /// Clause 5.7.1.5: an `omit` naming every member the entity has, `id` and
    /// `type` included, leaves nothing.
    #[test]
    fn an_omit_naming_every_member_leaves_nothing() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Id, ProjectionTerm::Type, attribute("name")],
        };

        assert_eq!(projection_remainder(&stripped(), &projection), ProjectionRemainder::Nothing);
    }

    /// The same `omit` leaves the entity standing while one attribute member
    /// survives it.
    #[test]
    fn an_omit_that_spares_an_attribute_leaves_the_entity() {
        let surviving = entity(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"}
        }));
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Id, ProjectionTerm::Type],
        };

        assert_eq!(projection_remainder(&surviving, &projection), ProjectionRemainder::Members);
    }

    /// Clause 5.2.4 keeps `location` in its own Entity member; it is an
    /// Attribute all the same, so an entity left with only that one is not
    /// left with nothing.
    #[test]
    fn a_surviving_geo_property_leaves_the_entity() {
        let located = entity(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }));
        let projection = Projection {
            pick: vec![attribute("location")],
            omit: Vec::new(),
        };

        assert_eq!(projection_remainder(&located, &projection), ProjectionRemainder::Members);
    }

    /// A request that named neither `pick` nor `omit` projects nothing away.
    #[test]
    fn an_empty_projection_always_leaves_the_entity() {
        assert_eq!(projection_remainder(&stripped(), &Projection::default()), ProjectionRemainder::Members);
    }
}
