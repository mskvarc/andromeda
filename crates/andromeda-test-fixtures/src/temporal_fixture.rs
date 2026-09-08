//! Reusable temporal payload construction.

use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::{Value, json};
use typed_builder::TypedBuilder;

/// Builder input for one temporal Property observation.
#[derive(Debug, TypedBuilder)]
pub struct TemporalPropertyFixture {
    /// Property name as it appears on the wire.
    attribute: IriBuf,
    /// Property value.
    value: Value,
    /// RFC 3339 observation timestamp.
    observed_at: DateTime<Utc>,
}

impl TemporalPropertyFixture {
    /// Render the attribute member and its temporal instance.
    #[must_use]
    pub fn into_member(self) -> (String, Value) {
        (
            self.attribute.to_string(),
            json!([{"type": "Property", "value": self.value, "observedAt": self.observed_at.to_rfc3339()}]),
        )
    }
}
