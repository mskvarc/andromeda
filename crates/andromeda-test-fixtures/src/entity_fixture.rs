//! Typed construction of reusable minimal entity payloads.

use iri_rs::IriBuf;
use serde_json::{Map, Value};
use typed_builder::TypedBuilder;

/// Builder input for a minimal valid NGSI-LD Entity.
#[derive(Debug, TypedBuilder)]
pub struct EntityFixture {
    /// Entity identifier.
    id: IriBuf,
    /// Entity type IRI or compact term represented at the wire boundary.
    entity_type: IriBuf,
    /// JSON-LD context IRIs.
    contexts: Vec<IriBuf>,
    /// Attribute members keyed by their wire names.
    #[builder(default = Map::new())]
    attributes: Map<String, Value>,
}

impl EntityFixture {
    /// Render the fixture as an NGSI-LD JSON object.
    pub fn into_value(self) -> Value {
        let mut object = self.attributes;
        object.insert("id".to_owned(), Value::String(self.id.to_string()));
        object.insert("type".to_owned(), Value::String(self.entity_type.to_string()));
        object.insert(
            "@context".to_owned(),
            Value::Array(self.contexts.into_iter().map(|context| Value::String(context.to_string())).collect()),
        );
        Value::Object(object)
    }
}
