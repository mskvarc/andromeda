//! Post-compaction array normalization.
//!
//! After JSON-LD compaction, single-element arrays may collapse to plain
//! objects. NGSI-LD temporal attributes must always be arrays, so
//! [`TemporalArrayNormalizer`] re-wraps them.

use serde_json::Value;
use std::mem::replace;

/// Entity members that are not temporal attribute arrays.
const RESERVED: &[&str] = &["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

/// Re-wraps temporal attributes that were collapsed by JSON-LD compaction.
#[derive(Debug)]
pub struct TemporalArrayNormalizer {
    /// Compacted temporal entity to normalize.
    payload: Value,
}

impl TemporalArrayNormalizer {
    /// Create a normalizer for the given compacted payload.
    pub const fn new(payload: Value) -> TemporalArrayNormalizer {
        TemporalArrayNormalizer { payload }
    }

    /// Re-wrap every temporal attribute that compaction collapsed.
    ///
    /// Re-wrapping cannot fail, so this answers with the value rather than a
    /// `Result` no caller could act on.
    pub fn render(self) -> Value {
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
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn normalize(value: Value) -> Value {
        TemporalArrayNormalizer::new(value).render()
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
