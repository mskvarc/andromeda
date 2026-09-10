//! The MQTT notification envelope.
//!
//! MQTT 3.1.1 has no headers and no user properties, so everything an HTTP
//! notification carries alongside the body has to travel inside the payload.
//! The envelope is `{"metadata": {...}, "body": ...}`, and MQTT 5 publishes it
//! as well as its own user properties so a client does not have to know which
//! version it is talking to.

use crate::{mqtt::metadata::transported_headers, request::DeliveryRequest};
use serde_json::{Map, Value};
use tracing::error;

/// The content-type member, named as the HTTP header it stands in for.
const CONTENT_TYPE_KEY: &str = "Content-Type";

/// Build the envelope a notification is published as.
///
/// The body is the notification [`Value`] the request carries, wrapped in the
/// `{metadata, body}` envelope and serialised once here — the transport never
/// parses a pre-serialised body back out.
pub fn build_envelope(request: &DeliveryRequest) -> Vec<u8> {
    let mut metadata = Map::new();
    metadata.insert(CONTENT_TYPE_KEY.to_string(), Value::String(request.content_type.as_str().to_string()));
    for (key, value) in transported_headers(request) {
        metadata.insert(key, Value::String(value));
    }

    let mut envelope = Map::new();
    envelope.insert("metadata".to_string(), Value::Object(metadata));
    // clone: the envelope owns its body, and the request keeps the notification
    envelope.insert("body".to_string(), (*request.payload).clone());
    let envelope = Value::Object(envelope);

    match serde_json::to_vec(&envelope) {
        Ok(bytes) => bytes,
        Err(error) => {
            error!(error = %error, "failed to serialize MQTT notification envelope");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        mqtt::envelope::build_envelope,
        request::{DeliveryRequest, NotificationDelivery},
    };
    use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use mediatype::MediaTypeBuf;
    use serde_json::{Value, json};
    use std::str::FromStr;
    use url::Url;

    fn request(accept: &str, receiver_info: Option<&[KeyValuePair]>, tenant: Option<&TenantName>) -> DeliveryRequest {
        let media = MediaTypeBuf::from_str(accept).expect("a well-formed media type");
        let endpoint = Url::parse("mqtt://broker:1883/topic").expect("a well-formed endpoint URI");
        let subscription = Uri::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        NotificationDelivery {
            payload: &json!({"id": "urn:ngsi-ld:Notification:1", "type": "Notification"}),
            endpoint_uri: &endpoint,
            accept: Some(&media),
            receiver_info,
            jsonld_context: None,
            tenant,
            snapshot: None,
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble()
    }

    fn envelope(request: &DeliveryRequest) -> Value {
        serde_json::from_slice(&build_envelope(request)).expect("the envelope parses")
    }

    #[test]
    fn the_body_is_the_notification_and_the_metadata_names_the_content_type() {
        let rendered = envelope(&request("application/json", None, None));
        pretty_assertions::assert_eq!(rendered["body"]["id"], json!("urn:ngsi-ld:Notification:1"));
        pretty_assertions::assert_eq!(rendered["metadata"]["Content-Type"], json!("application/json"));
    }

    #[test]
    fn the_envelope_carries_the_notification_value_verbatim_as_its_body() {
        // The body is the notification `Value` the request holds, not a re-parse
        // of pre-serialised bytes, so it round-trips through the envelope
        // unchanged — the same output the old parse-then-wrap produced.
        let req = request("application/json", None, None);
        let rendered = envelope(&req);
        pretty_assertions::assert_eq!(rendered["body"], (*req.payload).clone());
    }

    #[test]
    fn a_json_notification_carries_the_context_link_in_its_metadata() {
        let rendered = envelope(&request("application/json", None, None));
        let link = rendered["metadata"]["Link"].as_str().expect("a Link member");
        assert!(link.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{link}");
    }

    #[test]
    fn a_json_ld_notification_carries_no_context_link_because_the_body_has_it() {
        let rendered = envelope(&request("application/ld+json", None, None));
        assert!(rendered["metadata"].get("Link").is_none());
    }

    #[test]
    fn receiver_info_and_the_tenant_appear_as_metadata_members_under_the_names_the_spec_spells() {
        let pairs = vec![KeyValuePair {
            key: "Authorization".into(),
            value: "Bearer token".into(),
        }];
        let name = TenantName::try_from("acme".to_string()).expect("a valid tenant name");
        let rendered = envelope(&request("application/json", Some(&pairs), Some(&name)));

        pretty_assertions::assert_eq!(rendered["metadata"]["Authorization"], json!("Bearer token"));
        pretty_assertions::assert_eq!(rendered["metadata"]["NGSILD-Tenant"], json!("acme"));
    }
}
