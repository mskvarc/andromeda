//! Reusable snapshot request construction.

use iri_rs::IriBuf;
use serde_json::{Value, json};
use typed_builder::TypedBuilder;

/// Builder input for a snapshot request.
#[derive(Debug, TypedBuilder)]
pub struct SnapshotFixture {
    /// Snapshot identifier.
    id: IriBuf,
    /// Entity identifiers included in the snapshot.
    entities: Vec<IriBuf>,
    /// Snapshot priority.
    priority: u8,
}

impl SnapshotFixture {
    /// Render the snapshot request as JSON.
    #[must_use]
    pub fn into_value(self) -> Value {
        json!({"id": self.id, "entities": self.entities, "priority": self.priority})
    }
}
