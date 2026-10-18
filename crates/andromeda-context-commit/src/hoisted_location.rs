//! Whether a write changes the geometry a store hoists out of an entity.
//!
//! A store indexes the default `location` instance (clause 4.7) beside the
//! stored document, so a geoQuery (clause 4.10) on the default `GeoProperty`
//! never reads the document. Projecting a geometry costs in
//! proportion to its positions, and a read-modify-write that touches another
//! attribute leaves the geometry exactly as the state it read held it: the
//! typed state shares the untouched attribute with the state it was computed
//! from. Recording that here lets a store whose search row already holds the
//! projection of the replaced state keep it rather than project the geometry
//! again.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use std::ptr;

/// Whether a write changes the hoisted geometry of the entity it replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryChange {
    /// The written state holds a geometry other than the replaced state's, or
    /// one where the replaced state held none, or none where it held one; and
    /// every write that replaces no read state (a creation, or a record built
    /// without the state it replaces) is one of these.
    Changed,
    /// The written state holds the very geometry the replaced state held, or
    /// neither holds one.
    Unchanged,
}

impl GeometryChange {
    /// The change from `previous` to `current`.
    ///
    /// A geometry the two states share (the untouched attribute of a
    /// read-modify-write) is recognised by its address without reading a
    /// position; one the write restated is compared position by position, so a
    /// client that sends the same geometry again changes nothing either.
    #[must_use]
    pub fn between(previous: Option<&GeometryForm>, current: Option<&GeometryForm>) -> GeometryChange {
        match (previous, current) {
            (None, None) => GeometryChange::Unchanged,
            (Some(previous), Some(current)) if ptr::eq(previous, current) || previous == current => GeometryChange::Unchanged,
            (Some(_), Some(_) | None) | (None, Some(_)) => GeometryChange::Changed,
        }
    }

    /// What replacing `previous` with `current` does to the default `location`
    /// geometry.
    #[must_use]
    pub fn of_location(previous: &EntityState, current: &EntityState) -> GeometryChange {
        GeometryChange::between(previous.location_geometry(), current.location_geometry())
    }
}

#[cfg(test)]
mod tests {
    use crate::hoisted_location::GeometryChange;
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use serde_json::{Value, json};
    use std::{ptr, sync::Arc};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const OPERATION_SPACE: &str = "https://uri.etsi.org/ngsi-ld/operationSpace";
    const SPEED: &str = "https://example.org/speed";

    fn point(longitude: f64, latitude: f64) -> Value {
        json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
            }]
        }])
    }

    fn property(value: i64) -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    /// The stored expanded document of a Vehicle holding the given members.
    fn vehicle_document(members: &[(&str, Value)]) -> Vec<u8> {
        let mut node = json!({"@id": "urn:ngsi-ld:Vehicle:A", "@type": ["https://example.org/Vehicle"]});
        if let Some(node) = node.as_object_mut() {
            node.extend(members.iter().map(|(name, value)| ((*name).to_owned(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::to_vec(&json!([node])).expect("JSON")
    }

    /// A Vehicle holding the given members.
    fn vehicle(members: &[(&str, Value)]) -> EntityState {
        EntityState::from_slice(&vehicle_document(members)).expect("a stored entity decodes")
    }

    /// Clause 5.6.2: updating `speed` leaves `location` as it was, and the
    /// updated state shares it with the state it was computed from.
    #[test]
    fn an_update_of_another_attribute_leaves_the_location_unchanged() {
        let previous = Arc::new(vehicle(&[(LOCATION, point(13.4, 52.5)), (SPEED, property(1))]));
        let mut current = EntityState::clone(&previous); // clone: shares every attribute, as a read-modify-write does
        let fragment = EntityDocument::from_slice(&vehicle_document(&[(SPEED, property(2))])).expect("a written fragment decodes");
        current.update_attributes(&fragment).expect("speed is updated");
        assert!(
            ptr::eq(
                previous.location_geometry().expect("a location"),
                current.location_geometry().expect("a location")
            ),
            "the untouched location is shared"
        );
        pretty_assertions::assert_eq!(GeometryChange::of_location(&previous, &current), GeometryChange::Unchanged);
    }

    #[test]
    fn a_moved_location_is_changed() {
        let previous = vehicle(&[(LOCATION, point(13.4, 52.5))]);
        let current = vehicle(&[(LOCATION, point(2.35, 48.85))]);
        pretty_assertions::assert_eq!(GeometryChange::of_location(&previous, &current), GeometryChange::Changed);
    }

    /// A geometry restated with the same positions is the same geometry, even
    /// though the two states do not share it.
    #[test]
    fn a_restated_identical_location_is_unchanged() {
        let previous = vehicle(&[(LOCATION, point(13.4, 52.5))]);
        let current = vehicle(&[(LOCATION, point(13.4, 52.5)), (SPEED, property(1))]);
        pretty_assertions::assert_eq!(GeometryChange::of_location(&previous, &current), GeometryChange::Unchanged);
    }

    #[test]
    fn a_location_added_or_removed_is_changed() {
        let without = vehicle(&[(SPEED, property(1))]);
        let with = vehicle(&[(LOCATION, point(1.0, 2.0))]);
        pretty_assertions::assert_eq!(GeometryChange::of_location(&without, &with), GeometryChange::Changed);
        pretty_assertions::assert_eq!(GeometryChange::of_location(&with, &without), GeometryChange::Changed);
    }

    /// A space geometry is not hoisted, so adding one leaves the hoisted
    /// geometry as it was.
    #[test]
    fn an_added_operation_space_leaves_the_location_unchanged() {
        let without = vehicle(&[(LOCATION, point(13.4, 52.5))]);
        let with = vehicle(&[(LOCATION, point(13.4, 52.5)), (OPERATION_SPACE, point(1.0, 2.0))]);
        pretty_assertions::assert_eq!(GeometryChange::of_location(&without, &with), GeometryChange::Unchanged);
    }
}
