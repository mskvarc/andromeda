//! Deriving a projected value from the neutral observation an entity mutation
//! carries.
//!
//! An entity mutation states what each attribute instance now holds as a
//! [`TemporalValue`], the backend-neutral shape both current state and Temporal
//! Evolution are written from. The projection takes the same observation and
//! keeps it in one typed column, so this is the single place a neutral value
//! becomes a projected one.

use crate::current_attribute_values::row::ProjectedAttributeValue;
use andromeda_ngsi_ld_model::temporal::TemporalValue;
use thiserror::Error;

/// A neutral observation value has no projected form.
#[derive(Debug, Error)]
pub enum ProjectedValueUnrepresentable {
    /// The observation ends the instance rather than giving it a value.
    ///
    /// The projection holds what an entity carries now, so a deletion removes
    /// the row instead of writing one; a caller that reaches this has mistaken
    /// a tombstone for a value.
    #[error("a deletion is not a projected value; the instance's row is removed instead")]
    Deletion,
    /// The observation is an Entity's Scope rather than an Attribute value.
    ///
    /// Clause 4.18 makes the Scope "a special scope Property" that clause 4.5.4
    /// gives no attribute family and no dataset dimension, so it has no row in
    /// a projection of Attribute instances: current state holds it on the
    /// entity's own scope paths instead.
    #[error("an entity scope is not an attribute value; it is projected on the entity's scope paths instead")]
    Scope,
}

impl TryFrom<&TemporalValue> for ProjectedAttributeValue {
    type Error = ProjectedValueUnrepresentable;

    fn try_from(value: &TemporalValue) -> Result<ProjectedAttributeValue, ProjectedValueUnrepresentable> {
        match value {
            TemporalValue::Number(number) => Ok(ProjectedAttributeValue::Number(number.clone())), // clone: the projected row owns its exact decimal.
            // clone: the projected row owns the value it writes.
            TemporalValue::String(text) => Ok(ProjectedAttributeValue::String(text.clone())),
            TemporalValue::Boolean(flag) => Ok(ProjectedAttributeValue::Boolean(*flag)),
            // clone: the projected row owns the geometry it writes.
            TemporalValue::Geometry(geometry) => Ok(ProjectedAttributeValue::Geometry(geometry.clone())),
            // clone: the projected row owns the structured value it writes.
            TemporalValue::Structured(structured) => Ok(ProjectedAttributeValue::Object(structured.clone())),
            TemporalValue::DateTime(instant) => Ok(ProjectedAttributeValue::DateTime(*instant)),
            // clone: the projected row owns the URI it writes.
            TemporalValue::Uri(uri) => Ok(ProjectedAttributeValue::Uri(uri.clone())),
            TemporalValue::Tombstone { .. } => Err(ProjectedValueUnrepresentable::Deletion),
            TemporalValue::Scopes(_) => Err(ProjectedValueUnrepresentable::Scope),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::{observed_value::ProjectedValueUnrepresentable, row::ProjectedAttributeValue};
    use andromeda_ngsi_ld_entity::json_literal::JsonLiteral;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_model::temporal::{TemporalValue, structured_value::StructuredValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn a_deletion_has_no_projected_value_at_all() {
        let deleted_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        assert!(matches!(
            ProjectedAttributeValue::try_from(&TemporalValue::Tombstone { deleted_at }),
            Err(ProjectedValueUnrepresentable::Deletion)
        ));
    }

    #[test]
    fn every_observed_shape_projects_into_its_own_typed_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let observed = [
            TemporalValue::Number(JsonNumber::try_from(1.5).expect("a finite number")),
            TemporalValue::String("text".to_owned()),
            TemporalValue::Boolean(true),
            TemporalValue::Geometry(Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("a point")),
            TemporalValue::Structured(StructuredValue::Json(JsonLiteral::new(json!({"a": 1})))),
            TemporalValue::DateTime(instant),
            TemporalValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        let mut kinds: Vec<i16> = observed
            .iter()
            .map(|value| {
                let projected = ProjectedAttributeValue::try_from(value).expect("a representable observation");
                ValueKind::code(projected.kind())
            })
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        pretty_assertions::assert_eq!(kinds.len(), observed.len());
        assert!(!kinds.contains(&ValueKind::code(ValueKind::Tombstone)));
    }

    /// The projection is written from the shape the number arrived in, so an
    /// integer the double format cannot name reaches the `NUMERIC` column as
    /// itself rather than as the nearest double.
    #[test]
    fn a_number_beyond_the_exact_double_range_keeps_its_arbitrary_precision_form() {
        let observed = TemporalValue::Number(JsonNumber::from(9_007_199_254_740_993_i64));
        let projected = ProjectedAttributeValue::try_from(&observed).expect("an exact integer");
        pretty_assertions::assert_eq!(projected.kind(), ValueKind::Number);
        pretty_assertions::assert_eq!(projected, ProjectedAttributeValue::Number(JsonNumber::from(9_007_199_254_740_993_i64)));
    }

    /// The observation carries the geometry already built, so the projection
    /// writes exactly that geometry: a hole, an elevated position and the
    /// ordinate count each position arrived with all reach the column.
    #[test]
    fn a_geometry_observation_projects_to_the_same_geometry() {
        let geometry = Geometry::from_geojson_value(&json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0, 12.5], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0, 12.5]],
                [[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [1.0, 1.0]]
            ]
        }))
        .expect("a valid Polygon with a hole");
        let projected = ProjectedAttributeValue::try_from(&TemporalValue::Geometry(geometry.clone())).expect("a representable observation"); // clone: the test compares the projection against what it projected
        pretty_assertions::assert_eq!(projected.kind(), ValueKind::Geometry);
        pretty_assertions::assert_eq!(projected, ProjectedAttributeValue::Geometry(geometry));
    }
}
