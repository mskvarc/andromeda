//! A JSON value a column or field holds as written by the codec that owns it.

use serde_json::value::RawValue;
use sqlx::types::Json;

/// The JSON text of a structured value, a geometry's members or an instance's
/// seals, exactly as its codec wrote it.
#[derive(Clone, Debug)]
pub struct StoredJson(Box<RawValue>);

impl StoredJson {
    /// Hold `json` as it was written.
    #[must_use]
    pub const fn new(json: Box<RawValue>) -> StoredJson {
        StoredJson(json)
    }

    /// The JSON text.
    #[must_use]
    pub fn get(&self) -> &str {
        self.0.get()
    }

    /// The JSON, for a bind that borrows it.
    #[must_use]
    pub fn as_raw(&self) -> &RawValue {
        &self.0
    }

    /// The JSON as the `jsonb` bind and decode wrapper carries it.
    #[must_use]
    pub fn into_json(self) -> Json<Box<RawValue>> {
        Json(self.0)
    }
}

impl From<Json<Box<RawValue>>> for StoredJson {
    fn from(json: Json<Box<RawValue>>) -> StoredJson {
        StoredJson(json.0)
    }
}

// Hand-written: `RawValue` has no equality, and two stored pieces are equal
// exactly when their codec wrote the same text.
impl PartialEq for StoredJson {
    fn eq(&self, other: &StoredJson) -> bool {
        self.get() == other.get()
    }
}
