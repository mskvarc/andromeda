//! Counting what a compact entity payload will cost before it is expanded.
//!
//! JSON-LD expansion multiplies a payload: every scalar of an attribute value
//! becomes a one-member `{"@value": ...}` object, and a `GeoProperty` geometry
//! nested coordinate lists besides. What decides that cost is visible on the
//! normalized compact payload, so the census walks it once, before the
//! expansion is attempted, and counts two things:
//!
//! - the positions of every `GeoProperty` geometry (ETSI GS CIM 009 v1.9.1
//!   clause 4.7), in its object form or encoded as a JSON string (clause
//!   4.7.2), each geometry against the position limit; a string is counted
//!   from its text before anything parses it;
//! - every other JSON value, against the entity's value limit: a geometry's
//!   `type`, `bbox`, foreign members and the lists its positions are nested
//!   in among them. A `JsonProperty`'s `json` member counts once whatever it
//!   holds: clause 4.5.24 makes it a JSON literal, which expansion keeps
//!   verbatim.
//!
//! The budget is taken per request: it bounds what one write makes the broker
//! expand, not what an entity grows to over many writes.
//!
//! The walk reads an entity, an entity fragment, a temporal entity whose
//! attributes are instance arrays, and a lone attribute fragment wrapped under
//! its name alike: an attribute is an instance object or an array of them, and
//! an instance's members other than those its family reserves are its
//! sub-attributes. A family's reserved members are values, never attributes,
//! so a `Property` whose value happens to look like a `GeoProperty` is counted
//! as the value it is.

use crate::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_geometry::geometry_census::{
    GeometryCensusError,
    GeometryCensusLimits,
    GeometryOverLimit,
    GeometryPositionLimit,
    compact_geometry_census,
    text_geometry_census,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use derive_more::Display;
use serde_json::{Map, Value};
use std::str::FromStr;
use thiserror::Error;

/// The budget a written entity payload is admitted under, before it is
/// expanded.
///
/// Every replica of a deployment runs the same budget: a load balancer may hand
/// any request to any of them, and an entity one admitted is read and deleted
/// by the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityPayloadBudget {
    /// The most JSON values the payload may hold outside its geometries.
    pub json_values: JsonValueLimit,
    /// The most positions any one geometry of the payload may hold.
    pub geometry_positions: GeometryPositionLimit,
}

/// What a payload's census found over its budget.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PayloadBudgetExceeded {
    /// The payload holds more JSON values outside its geometries' positions
    /// than the entity limit admits.
    #[error(
        "the entity holds more than the {limit} JSON values this broker accepts in one entity, geometry positions and JSON literals aside; send its values over several entities or requests"
    )]
    EntityJsonValues {
        /// The limit the payload exceeds.
        limit: JsonValueLimit,
    },
    /// A geometry of the payload holds more positions than the limit admits.
    #[error("{owner} holds more than the {limit} positions this broker accepts in one geometry; send a simplified geometry")]
    GeometryPositions {
        /// The attribute whose geometry exceeds the limit.
        owner: GeometryOwner,
        /// The limit the geometry exceeds.
        limit: GeometryPositionLimit,
    },
}

/// The attribute a refused geometry belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Display)]
pub enum GeometryOwner {
    /// A geometry of the attribute of this name.
    #[display("the geometry of the attribute '{_0}'")]
    Attribute(AttributeName),
    /// A geometry of an attribute whose name is no attribute name, which the
    /// attribute admission refuses on its own account.
    #[display("a geometry")]
    Unnamed,
}

impl EntityPayloadBudget {
    /// The limits a geometry standing on its own is admitted under: as many
    /// positions as one geometry of an entity, and besides them as many
    /// other values as one entity.
    #[must_use]
    pub const fn geometry_census_limits(self) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: self.geometry_positions,
            values: self.json_values,
        }
    }

    /// Admit a normalized compact payload whose census stays within this
    /// budget.
    ///
    /// # Errors
    ///
    /// Returns the first [`PayloadBudgetExceeded`] the census meets: a
    /// geometry over the position limit, or the value count passing the
    /// entity limit.
    pub fn admit(&self, payload: &Value) -> Result<(), PayloadBudgetExceeded> {
        census_entity_payload(payload, *self)
    }

    /// Admit the normalized bare Attribute Fragment clauses 5.6.4 and 5.6.19
    /// write to `attribute`, whose census stays within this budget.
    ///
    /// # Errors
    ///
    /// Returns the first [`PayloadBudgetExceeded`] the census meets, on the
    /// same terms as [`EntityPayloadBudget::admit`].
    pub fn admit_attribute(&self, attribute: &AttributeName, fragment: &Value) -> Result<(), PayloadBudgetExceeded> {
        Census::new(*self).attribute(attribute.as_str(), fragment)
    }
}

/// Take the census of a normalized compact payload against `limits`.
fn census_entity_payload(payload: &Value, limits: EntityPayloadBudget) -> Result<(), PayloadBudgetExceeded> {
    let census = Census::new(limits);
    match payload {
        Value::Object(entity) => {
            census.count()?;
            entity.iter().try_for_each(|(member, value)| {
                if ENTITY_MEMBERS.contains(&member.as_str()) {
                    census.values_of(value)
                } else {
                    census.attribute(member, value)
                }
            })
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => census.values_of(payload),
    }
}

/// The running count of one payload's census.
struct Census {
    /// The limits the payload is counted against.
    limits: EntityPayloadBudget,
    /// The JSON values counted so far outside geometry positions.
    values: ValueTally,
}

impl Census {
    /// A census of nothing yet, under `limits`.
    const fn new(limits: EntityPayloadBudget) -> Census {
        Census {
            limits,
            values: ValueTally::new(limits.json_values),
        }
    }

    /// The refusal of a payload past the entity value limit.
    const fn too_many_values(&self) -> PayloadBudgetExceeded {
        PayloadBudgetExceeded::EntityJsonValues {
            limit: self.limits.json_values,
        }
    }

    /// Count one JSON value, refusing once the count passes the entity limit.
    fn count(&self) -> Result<(), PayloadBudgetExceeded> {
        self.values.admit_one().map_err(|_exceeded| self.too_many_values())
    }

    /// Count `value` and every value below it.
    fn values_of(&self, value: &Value) -> Result<(), PayloadBudgetExceeded> {
        self.count()?;
        match value {
            Value::Array(items) => items.iter().try_for_each(|item| self.values_of(item)),
            Value::Object(members) => members.values().try_for_each(|member| self.values_of(member)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }

    /// Count the attribute `name` holds: one instance, several, or a value the
    /// normalization left as it found it.
    fn attribute(&self, name: &str, slot: &Value) -> Result<(), PayloadBudgetExceeded> {
        match slot {
            Value::Object(instance) => self.instance(name, instance),
            Value::Array(instances) => {
                self.count()?;
                instances.iter().try_for_each(|instance| match instance {
                    Value::Object(instance) => self.instance(name, instance),
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => self.values_of(instance),
                })
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => self.values_of(slot),
        }
    }

    /// Count one instance of the attribute `name`.
    fn instance(&self, name: &str, instance: &Map<String, Value>) -> Result<(), PayloadBudgetExceeded> {
        self.count()?;
        let family = declared_family(instance);
        instance.iter().try_for_each(|(member, value)| match family {
            Some(AttributeType::GeoProperty) if is_geometry_member(member) => self.geometry_member(name, member, value),
            Some(AttributeType::JsonProperty) if is_json_literal_member(member) => self.count(),
            Some(family) if member == "type" || family.reserved_fields().contains(&member.as_str()) => self.values_of(value),
            Some(_) => self.attribute(member, value),
            // An instance declaring no family is refused by the attribute
            // admission; until then nothing in it is taken for a geometry.
            None => self.values_of(value),
        })
    }

    /// Count a geometry-bearing member of a `GeoProperty` instance: one
    /// geometry, or the `values` array of several (clause 4.5.9).
    fn geometry_member(&self, name: &str, member: &str, value: &Value) -> Result<(), PayloadBudgetExceeded> {
        match value {
            Value::Array(geometries) if member == compact_properties::VALUES => {
                self.count()?;
                geometries.iter().try_for_each(|geometry| self.geometry(name, geometry))
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => self.geometry(name, value),
        }
    }

    /// Count one geometry: its positions against the position limit, and
    /// every other value it holds against the entity limit.
    ///
    /// A geometry encoded as a JSON string (clause 4.7.2) is one value as
    /// written and counted from its text besides, before anything parses it.
    /// A string that is no JSON text is no geometry, which the geometry
    /// admission refuses; so is any other value that is no object.
    fn geometry(&self, name: &str, geometry: &Value) -> Result<(), PayloadBudgetExceeded> {
        let positions = self.limits.geometry_positions;
        let counted = match geometry {
            Value::Object(_) => compact_geometry_census(geometry, positions, &self.values),
            Value::String(text) => {
                self.count()?;
                text_geometry_census(text, positions, &self.values)
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => return self.values_of(geometry),
        };
        match counted {
            Ok(_) | Err(GeometryCensusError::Syntax(_)) => Ok(()),
            Err(GeometryCensusError::OverLimit(GeometryOverLimit::Values { .. })) => Err(self.too_many_values()),
            Err(GeometryCensusError::OverLimit(GeometryOverLimit::Positions { limit })) => Err(PayloadBudgetExceeded::GeometryPositions {
                owner: AttributeName::from_str(name).map_or(GeometryOwner::Unnamed, GeometryOwner::Attribute),
                limit,
            }),
        }
    }
}

/// The attribute family an instance declares, by its compact name or its IRI.
fn declared_family(instance: &Map<String, Value>) -> Option<AttributeType> {
    let declared = instance.get("type")?.as_str()?;
    AttributeType::from_str(declared).ok().or_else(|| AttributeType::from_expanded_iri(declared))
}

/// Whether a `GeoProperty` member holds geometries.
fn is_geometry_member(member: &str) -> bool {
    [compact_properties::VALUE, compact_properties::PREVIOUS_VALUE, compact_properties::VALUES].contains(&member)
}

/// Whether a `JsonProperty` member holds a JSON literal.
fn is_json_literal_member(member: &str) -> bool {
    [compact_properties::JSON, compact_properties::PREVIOUS_JSON, compact_properties::JSONS].contains(&member)
}

#[cfg(test)]
mod tests {
    use crate::payload_census::{EntityPayloadBudget, GeometryOwner, PayloadBudgetExceeded};
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// Limits of `values` JSON values and `positions` positions.
    fn limits(values: usize, positions: usize) -> EntityPayloadBudget {
        EntityPayloadBudget {
            json_values: JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive test limit")),
            geometry_positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive test limit")),
        }
    }

    /// Whether `payload` passes the census at exactly `values` values and
    /// fails it one below.
    fn holds_exactly(payload: &Value, values: usize) -> bool {
        limits(values, usize::MAX).admit(payload).is_ok() && limits(values - 1, usize::MAX).admit(payload).is_err()
    }

    /// A closed ring of `positions` positions.
    fn ring(positions: usize) -> Value {
        let mut points: Vec<Value> = (0_u32..).take(positions - 1).map(|index| json!([f64::from(index) * 0.001, 0.5])).collect();
        points.push(json!([0.0, 0.5]));
        Value::Array(points)
    }

    /// A `location` `GeoProperty` instance holding a Polygon of `positions`.
    fn located(positions: usize) -> Value {
        json!({"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [ring(positions)]}})
    }

    #[test]
    fn an_entity_of_scalars_counts_every_member_value() {
        // entity, id, type, attribute instance, its type, its value
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "speed": {"type": "Property", "value": 1}});
        assert!(holds_exactly(&payload, 6), "{payload}");
    }

    #[test]
    fn geometry_coordinates_count_as_positions_not_as_entity_values() {
        // entity, id, type, instance, its type, the geometry, its type, the
        // ring list and the ring
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": located(5)});
        assert!(holds_exactly(&payload, 9), "{payload}");
        assert!(limits(9, 5).admit(&payload).is_ok());
        assert!(limits(9, 4).admit(&payload).is_err());
    }

    /// IETF RFC 7946 sections 5 and 6.1: a geometry's `bbox` and foreign
    /// members are carried by no position, and cost the values they hold.
    #[test]
    fn a_geometry_bbox_and_foreign_members_count_as_entity_values() {
        let geometry = json!({"type": "Point", "coordinates": [0, 0], "bbox": [0, 0, 1, 1], "foreign": (0..20).collect::<Vec<u32>>()});
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": {"type": "GeoProperty", "value": geometry}});
        // entity, id, type, instance, its type, the geometry, its type, the
        // bbox and its four numbers, the foreign list and its twenty numbers
        assert!(holds_exactly(&payload, 33), "{payload}");
    }

    /// Clause 4.7.2: a geometry string is counted from its text, foreign
    /// members included, before it is parsed.
    #[test]
    fn a_string_encoded_geometry_counts_its_foreign_members_as_entity_values() {
        let text = json!({"type": "Point", "coordinates": [0, 0], "foreign": (0..20).collect::<Vec<u32>>()}).to_string();
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": {"type": "GeoProperty", "value": text}});
        // entity, id, type, instance, its type, the string, the geometry, its
        // type, the foreign list and its twenty numbers
        assert!(holds_exactly(&payload, 29), "{payload}");
        pretty_assertions::assert_eq!(
            limits(28, 5).admit(&payload),
            Err(PayloadBudgetExceeded::EntityJsonValues {
                limit: limits(28, 5).json_values
            })
        );
    }

    /// A string that is no JSON text is no geometry: the census leaves its
    /// refusal to the geometry admission.
    #[test]
    fn a_geometry_string_that_is_not_json_is_left_to_the_admission() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": {"type": "GeoProperty", "value": "not { json"}});
        assert!(limits(100, 5).admit(&payload).is_ok());
    }

    #[test]
    fn a_json_property_literal_is_not_counted() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "config": {"type": "JsonProperty", "json": (0..1000).collect::<Vec<u32>>()}});
        assert!(holds_exactly(&payload, 6), "{payload}");
    }

    #[test]
    fn a_geo_property_sub_attribute_is_budgeted() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "speed": {"type": "Property", "value": 1, "where": located(6)}});
        pretty_assertions::assert_eq!(
            limits(100, 5).admit(&payload),
            Err(PayloadBudgetExceeded::GeometryPositions {
                owner: GeometryOwner::Attribute("where".parse().expect("a name")),
                limit: limits(100, 5).geometry_positions,
            })
        );
    }

    #[test]
    fn every_instance_of_a_multi_instance_geo_property_is_budgeted() {
        let mut second = located(6);
        second["datasetId"] = json!("urn:ngsi-ld:Dataset:b");
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": [located(5), second]});
        let refusal = limits(100, 5).admit(&payload).expect_err("the second instance is over");
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "the geometry of the attribute 'location' holds more than the 5 positions this broker accepts in one geometry; send a simplified geometry"
        );
    }

    /// Clause 4.7.2: a geometry encoded as a JSON string is budgeted as the
    /// geometry it encodes.
    #[test]
    fn a_string_encoded_geometry_is_budgeted() {
        let text = json!({"type": "Polygon", "coordinates": [ring(6)]}).to_string();
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": {"type": "GeoProperty", "value": text}});
        assert!(limits(100, 5).admit(&payload).is_err());
        assert!(limits(100, 6).admit(&payload).is_ok());
    }

    #[test]
    fn a_previous_value_geometry_is_budgeted() {
        let mut instance = located(4);
        instance["previousValue"] = json!({"type": "Polygon", "coordinates": [ring(6)]});
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": instance});
        assert!(limits(100, 5).admit(&payload).is_err());
    }

    /// The census reads the instance arrays of a temporal entity
    /// (clause 4.5.9) as it reads a multi-instance attribute.
    #[test]
    fn a_temporal_entity_instance_array_is_budgeted() {
        let instance = |positions: usize, at: &str| {
            let mut instance = located(positions);
            instance["observedAt"] = json!(at);
            instance
        };
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": [
            instance(5, "2026-01-01T00:00:00Z"),
            instance(5, "2026-01-01T00:01:00Z"),
            instance(7, "2026-01-01T00:02:00Z"),
        ]});
        assert!(limits(100, 6).admit(&payload).is_err());
        assert!(limits(100, 7).admit(&payload).is_ok());
    }

    /// The expanded IRI of the family reads like its compact name.
    #[test]
    fn a_geo_property_named_by_its_iri_is_budgeted() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": {"type": "https://uri.etsi.org/ngsi-ld/GeoProperty", "value": {"type": "Polygon", "coordinates": [ring(6)]}}});
        assert!(limits(100, 5).admit(&payload).is_err());
    }

    #[test]
    fn an_entity_past_the_value_limit_is_refused() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "n": {"type": "Property", "value": (0..11).collect::<Vec<u32>>()}});
        pretty_assertions::assert_eq!(
            limits(10, 5).admit(&payload),
            Err(PayloadBudgetExceeded::EntityJsonValues {
                limit: limits(10, 5).json_values
            })
        );
    }

    /// A family's reserved members are values: a Property whose value looks
    /// like a `GeoProperty` is counted as values, never as a geometry.
    #[test]
    fn a_property_value_shaped_like_a_geo_property_is_counted_as_values() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "shape": {"type": "Property", "value": located(6)}});
        assert!(limits(1000, 5).admit(&payload).is_ok());
        assert!(limits(20, usize::MAX).admit(&payload).is_err());
    }

    /// A Partial Attribute Update fragment is budgeted as the attribute it
    /// writes.
    #[test]
    fn the_attribute_admission_budgets_a_bare_fragment() {
        let name: AttributeName = "location".parse().expect("a name");
        pretty_assertions::assert_eq!(
            limits(100, 5).admit_attribute(&name, &located(6)),
            Err(PayloadBudgetExceeded::GeometryPositions {
                owner: GeometryOwner::Attribute(name.clone()), // clone: the name is asserted against below
                limit: limits(100, 5).geometry_positions,
            })
        );
        assert!(limits(100, 6).admit_attribute(&name, &located(6)).is_ok());
        assert!(limits(3, 6).admit_attribute(&name, &json!({"type": "Property", "value": [1, 2]})).is_err());
    }

    #[test]
    fn a_payload_at_both_limits_is_admitted() {
        let payload = json!({"id": "urn:ngsi-ld:T:1", "type": "T", "location": located(5)});
        assert!(limits(9, 5).admit(&payload).is_ok());
    }
}
