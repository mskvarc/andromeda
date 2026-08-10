//! Typed construction of reusable Context Source Registration payloads.

use iri_rs::IriBuf;
use serde_json::{Value, json};
use typed_builder::TypedBuilder;
use url::Url;

/// Builder input for a minimal Context Source Registration.
#[derive(Debug, TypedBuilder)]
pub struct RegistrationFixture {
    /// Registration identifier.
    id: IriBuf,
    /// Entity type served by the Context Source.
    entity_type: IriBuf,
    /// Context Source endpoint.
    endpoint: Url,
    /// JSON-LD context IRIs.
    contexts: Vec<IriBuf>,
}

impl RegistrationFixture {
    /// Render the fixture as an NGSI-LD JSON object.
    pub fn into_value(self) -> Value {
        json!({
            "id": self.id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": self.entity_type}]}],
            "endpoint": self.endpoint,
            "@context": self.contexts,
        })
    }
}
