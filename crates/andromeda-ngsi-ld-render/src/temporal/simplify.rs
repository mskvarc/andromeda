//! Temporal simplify renderer: deserializes a compacted temporal entity value
//! and produces its key-values simplified form.

use crate::simplify::simplify_entity;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use serde_json::Value;
use thiserror::Error;

/// A compacted temporal representation could not be simplified.
#[derive(Debug, Error)]
pub enum TemporalSimplifyError {
    /// The input is not a valid compacted entity representation.
    #[error("temporal simplification received an invalid entity: {0}")]
    InvalidEntity(serde_json::Error),
    /// A collection result was requested as one entity.
    #[error("temporal simplification expected one entity, but produced a collection")]
    ExpectedSingle,
    /// A single result was requested as a collection.
    #[error("temporal simplification expected a collection, but produced one entity")]
    ExpectedCollection,
}

/// Payload shape for [`TemporalSimplifyRenderer`].
#[derive(Debug)]
pub enum TemporalSimplifyPayload {
    /// Simplify a single temporal entity value.
    Single(Value),
    /// Simplify each value in a collection of temporal entities.
    Collection(Vec<Value>),
}

/// Matching output shape for [`TemporalSimplifyRenderer`].
#[derive(Debug)]
pub enum TemporalSimplifyOutput {
    /// Simplified single temporal entity value.
    Single(Value),
    /// Simplified collection of temporal entity values.
    Collection(Vec<Value>),
}

impl TemporalSimplifyOutput {
    /// Extract the `Single` variant's value.
    ///
    /// Returns an error if the variant is `Collection`.
    pub fn into_single(self) -> Result<Value, TemporalSimplifyError> {
        match self {
            TemporalSimplifyOutput::Single(value) => Ok(value),
            TemporalSimplifyOutput::Collection(_) => Err(TemporalSimplifyError::ExpectedSingle),
        }
    }

    /// Extract the `Collection` variant's values.
    ///
    /// Returns an error if the variant is `Single`.
    pub fn into_collection(self) -> Result<Vec<Value>, TemporalSimplifyError> {
        match self {
            TemporalSimplifyOutput::Collection(values) => Ok(values),
            TemporalSimplifyOutput::Single(_) => Err(TemporalSimplifyError::ExpectedCollection),
        }
    }
}

/// Simplifies temporal entity values to key-values form by round-tripping
/// through the [`Entity`] domain type
/// and calling its `simplify` method.
#[derive(Debug)]
pub struct TemporalSimplifyRenderer {
    /// Shape-preserving temporal payload to simplify.
    payload: TemporalSimplifyPayload,
}

impl TemporalSimplifyRenderer {
    /// Create a renderer for the given payload.
    #[must_use]
    pub const fn new(payload: TemporalSimplifyPayload) -> TemporalSimplifyRenderer {
        TemporalSimplifyRenderer { payload }
    }
}

impl TemporalSimplifyRenderer {
    /// Simplify the payload, whichever shape it carries.
    ///
    /// # Errors
    ///
    /// Returns an error when a value does not deserialise as an entity.
    pub fn render(self) -> Result<TemporalSimplifyOutput, TemporalSimplifyError> {
        match self.payload {
            TemporalSimplifyPayload::Single(value) => Ok(TemporalSimplifyOutput::Single(simplify_value(value)?)),
            TemporalSimplifyPayload::Collection(values) => {
                let simplified = values.into_iter().map(simplify_value).collect::<Result<Vec<_>, _>>()?;
                Ok(TemporalSimplifyOutput::Collection(simplified))
            }
        }
    }
}

/// Simplify one compacted temporal entity.
fn simplify_value(entity_value: Value) -> Result<Value, TemporalSimplifyError> {
    let entity: Entity = deserialize_exact(&entity_value).map_err(TemporalSimplifyError::InvalidEntity)?;
    Ok(simplify_entity(&entity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render_single(value: Value) -> Value {
        match TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(value)).render().expect("render") {
            TemporalSimplifyOutput::Single(v) => v,
            TemporalSimplifyOutput::Collection(_) => panic!("expected single"),
        }
    }

    fn render_collection(values: Vec<Value>) -> Vec<Value> {
        match TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(values))
            .render()
            .expect("render")
        {
            TemporalSimplifyOutput::Collection(v) => v,
            TemporalSimplifyOutput::Single(_) => panic!("expected collection"),
        }
    }

    #[test]
    fn simplify_temporal_value_with_property() {
        let entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 100
            }
        });
        let result = render_single(entity);
        pretty_assertions::assert_eq!(result["id"], "urn:ngsi-ld:Vehicle:1");
        pretty_assertions::assert_eq!(result["type"], "Vehicle");
        pretty_assertions::assert_eq!(result["speed"], 100);
    }

    #[test]
    fn simplify_temporal_value_with_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "isParked": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:1"
            }
        });
        let result = render_single(entity);
        pretty_assertions::assert_eq!(result["isParked"], "urn:ngsi-ld:Parking:1");
    }

    #[test]
    fn simplify_temporal_value_invalid_json_returns_error() {
        let not_entity = json!({"not": "an entity"});
        let outcome = TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(not_entity)).render();
        assert!(outcome.is_err());
    }

    #[test]
    fn simplify_temporal_values_empty_vec() {
        let result = render_collection(Vec::new());
        assert_eq!(result, [] as [serde_json::Value; 0]);
    }

    #[test]
    fn simplify_temporal_values_multiple_entities() {
        let entities = vec![
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 100 }
            }),
            json!({
                "id": "urn:ngsi-ld:Vehicle:2",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 200 }
            }),
        ];
        let result = render_collection(entities);
        pretty_assertions::assert_eq!(result.len(), 2);
        pretty_assertions::assert_eq!(result[0]["speed"], 100);
        pretty_assertions::assert_eq!(result[1]["speed"], 200);
    }

    #[test]
    fn simplify_temporal_values_propagates_error() {
        let entities = vec![
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 100 }
            }),
            json!({"not": "an entity"}),
        ];
        let outcome = TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities)).render();
        assert!(outcome.is_err());
    }
}
