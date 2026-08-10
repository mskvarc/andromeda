//! Typed construction of reusable minimal subscription payloads.

use iri_rs::IriBuf;
use serde_json::{Value, json};
use typed_builder::TypedBuilder;
use url::Url;

/// Builder input for a minimal valid NGSI-LD Subscription.
#[derive(Debug, TypedBuilder)]
pub struct SubscriptionFixture {
    /// Optional subscription identifier.
    #[builder(default)]
    id: Option<IriBuf>,
    /// Entity type selected by the subscription.
    entity_type: IriBuf,
    /// Notification callback URL.
    endpoint: Url,
    /// JSON-LD context IRIs.
    contexts: Vec<IriBuf>,
}

impl SubscriptionFixture {
    /// Render the fixture as an NGSI-LD JSON object.
    pub fn into_value(self) -> Value {
        let mut value = json!({
            "type": "Subscription",
            "entities": [{"type": self.entity_type}],
            "notification": {"endpoint": {"uri": self.endpoint}},
            "@context": self.contexts,
        });
        if let Some(id) = self.id {
            value["id"] = Value::String(id.to_string());
        }
        value
    }
}
