//! Post-compaction array normalization.
//!
//! After JSON-LD compaction, single-element arrays may collapse to plain
//! objects. NGSI-LD temporal attributes must always be arrays, so
//! [`TemporalArrayNormalizer`] re-wraps them.

use crate::{core::error::AndromedaError, renderer::Renderer};
use serde_json::Value;
use std::mem::replace;

const RESERVED: &[&str] = &["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

/// Re-wraps temporal attributes that were collapsed by JSON-LD compaction.
#[derive(Debug)]
pub struct TemporalArrayNormalizer {
    payload: Value,
}

impl TemporalArrayNormalizer {
    /// Create a normalizer for the given compacted payload.
    pub const fn new(payload: Value) -> TemporalArrayNormalizer {
        TemporalArrayNormalizer { payload }
    }
}

impl Renderer for TemporalArrayNormalizer {
    type Output = Value;

    fn render(self) -> Result<Value, AndromedaError> {
        let mut payload = self.payload;
        if let Some(obj) = payload.as_object_mut() {
            for (key, value) in obj.iter_mut() {
                if RESERVED.contains(&key.as_str()) {
                    continue;
                }
                if !value.is_array() {
                    let taken = replace(value, Value::Null);
                    *value = Value::Array(vec![taken]);
                }
            }
        }
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn normalize(value: Value) -> Value {
        TemporalArrayNormalizer::new(value).render().expect("render")
    }

    #[test]
    fn wraps_objects() {
        let value = json!({"id": "urn:x", "type": "T", "speed": {"type": "Property", "value": 25}});
        let result = normalize(value);
        assert!(result["speed"].is_array());
        pretty_assertions::assert_eq!(result["speed"][0]["value"], 25);
    }

    #[test]
    fn leaves_arrays_alone() {
        let value = json!({"id": "urn:x", "type": "T", "speed": [{"type": "Property", "value": 25}]});
        let original = value.clone();
        let result = normalize(value);
        pretty_assertions::assert_eq!(result, original);
    }

    #[test]
    fn skips_reserved_keys() {
        let value = json!({"id": "urn:x", "type": "T", "createdAt": "2025-01-01T00:00:00Z"});
        let result = normalize(value);
        assert!(result["createdAt"].is_string());
    }
}
