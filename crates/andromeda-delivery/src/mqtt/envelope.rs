//! The MQTT notification envelope.
//!
//! MQTT 3.1.1 has no headers and no user properties, so everything an HTTP
//! notification carries alongside the body has to travel inside the payload.
//! The envelope is `{"metadata": {...}, "body": ...}`, and MQTT 5 publishes it
//! as well as its own user properties so a client does not have to know which
//! version it is talking to.

use crate::{error::TransportError, mqtt::metadata::transported_headers, request::DeliveryRequest};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// The content-type member, named as the HTTP header it stands in for.
const CONTENT_TYPE_KEY: &str = "Content-Type";

/// The `{metadata, body}` envelope one notification is published as.
#[derive(Serialize)]
struct Envelope<'a> {
    /// What an HTTP notification carries in its headers, one member per name.
    metadata: BTreeMap<String, String>,
    /// The written notification, as the HTTP delivery sends it.
    body: &'a RawValue,
}

/// Build the envelope a notification is published as.
///
/// The body is the notification the request carries, written once by its
/// sender and embedded here as it was written.
///
/// # Errors
///
/// Returns [`TransportError::PayloadSerialization`] when the envelope cannot
/// be written.
pub fn build_envelope(request: &DeliveryRequest) -> Result<Vec<u8>, TransportError> {
    let mut metadata = BTreeMap::new();
    metadata.insert(CONTENT_TYPE_KEY.to_string(), request.content_type.as_str().to_string());
    metadata.extend(transported_headers(request));
    let envelope = Envelope {
        metadata,
        body: request.body.json(),
    };
    serde_json::to_vec(&envelope).map_err(|source| TransportError::PayloadSerialization {
        endpoint: request.endpoint_uri.clone(), // clone: the error owns the endpoint it names
        source,
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        mqtt::envelope::build_envelope,
        notification_body::NotificationBody,
        notification_context::NotificationContext,
        request::{DeliveryRequest, NotificationDelivery},
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, tenant::TenantName};
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use mediatype::MediaTypeBuf;
    use serde::Deserialize;
    use serde_json::{Value, json, value::RawValue};
    use std::{collections::BTreeMap, str::FromStr};
    use url::Url;

    fn request(accept: &str, receiver_info: Option<&[KeyValuePair]>, tenant: Option<&TenantName>) -> DeliveryRequest {
        notification(
            &json!({"id": "urn:ngsi-ld:Notification:1", "type": "Notification"}),
            accept,
            receiver_info,
            tenant,
        )
    }

    /// The MQTT delivery of the notification `payload` to an endpoint whose
    /// `accept` is `accept`.
    fn notification(payload: &Value, accept: &str, receiver_info: Option<&[KeyValuePair]>, tenant: Option<&TenantName>) -> DeliveryRequest {
        let media = MediaTypeBuf::from_str(accept).expect("a well-formed media type");
        let endpoint = Url::parse("mqtt://broker:1883/topic").expect("a well-formed endpoint URI");
        let subscription = Identifier::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id");
        let context = NotificationContext::core();
        let id = Identifier::from_str("urn:ngsi-ld:Notification:1").expect("a well-formed notification id");
        NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .accept(Some(&media))
            .receiver_info(receiver_info)
            .context(&context)
            .tenant(tenant)
            .subscription_id(&subscription)
            .build()
            .assemble(NotificationBody::written(&id, payload).expect("a written notification"))
    }

    /// Table 5.3.1-1 over MQTT (clause 7.2): the body is the document the
    /// HTTP delivery sends, written once, an `application/geo+json`
    /// notification's `FeatureCollection` included, and the metadata of a body
    /// carrying its context names no `Link`.
    #[test]
    fn a_geo_json_notification_body_is_the_document_the_http_delivery_sends() {
        let payload = json!({
            "@context": NGSI_LD_CORE_CONTEXT_URL.as_str(),
            "id": "urn:ngsi-ld:Notification:1",
            "type": "Notification",
            "data": {
                "type": "FeatureCollection",
                "@context": NGSI_LD_CORE_CONTEXT_URL.as_str(),
                "features": [{"type": "Feature", "id": "urn:ngsi-ld:Building:1", "geometry": null, "properties": {"type": "Building"}}]
            }
        });

        let request = notification(&payload, "application/geo+json", None, None);
        let rendered = envelope(&request);

        pretty_assertions::assert_eq!(rendered["metadata"]["Content-Type"], json!("application/geo+json"));
        pretty_assertions::assert_eq!(rendered["body"], payload);
        assert!(rendered["metadata"].get("Link").is_none());
    }

    fn envelope(request: &DeliveryRequest) -> Value {
        serde_json::from_slice(&build_envelope(request).expect("the envelope is written")).expect("the envelope parses")
    }

    #[test]
    fn the_body_is_the_notification_and_the_metadata_names_the_content_type() {
        let rendered = envelope(&request("application/json", None, None));
        pretty_assertions::assert_eq!(rendered["body"]["id"], json!("urn:ngsi-ld:Notification:1"));
        pretty_assertions::assert_eq!(rendered["metadata"]["Content-Type"], json!("application/json"));
    }

    /// The body is the written notification byte for byte: the envelope
    /// embeds the text the HTTP delivery sends rather than writing the document
    /// again.
    #[test]
    fn the_envelope_embeds_the_written_notification_byte_for_byte() {
        let req = request("application/json", None, None);
        let written = String::from_utf8(build_envelope(&req).expect("the envelope is written")).expect("UTF-8");
        assert!(written.ends_with(&format!(r#""body":{}}}"#, req.body.text())), "{written}");
    }

    /// Clause 7.2 (MQTT binding): the envelope's `body` member is the very
    /// document the HTTP delivery sends as its body, read back from the
    /// envelope as raw text, and `metadata` holds what HTTP carries in headers.
    #[test]
    fn the_mqtt_envelope_wraps_the_written_body() {
        #[derive(Deserialize)]
        struct ReadEnvelope<'a> {
            #[serde(borrow)]
            metadata: BTreeMap<&'a str, &'a str>,
            #[serde(borrow)]
            body: &'a RawValue,
        }

        let req = notification(
            &json!({"id": "urn:ngsi-ld:Notification:1", "type": "Notification", "data": [{"id": "urn:ngsi-ld:Building:1", "type": "Building", "n": 1.10}]}),
            "application/ld+json",
            None,
            None,
        );
        let published = build_envelope(&req).expect("the envelope is written");
        let read: ReadEnvelope<'_> = serde_json::from_slice(&published).expect("the envelope parses");

        pretty_assertions::assert_eq!(read.body.get(), req.body.text());
        pretty_assertions::assert_eq!(read.metadata.get("Content-Type"), Some(&"application/ld+json"));
        pretty_assertions::assert_eq!(read.metadata.keys().copied().collect::<Vec<_>>(), vec!["Content-Type"]);
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

    /// The envelope's `metadata` holds one member per name, so a `receiverInfo`
    /// `Link` joins the `@context` link in one member instead of replacing it.
    #[test]
    fn a_receiver_info_link_joins_the_context_link_in_one_member() {
        let receiver_link = r#"<https://example.org/doc>; rel="describedby"; title="a, b""#;
        let pairs = vec![KeyValuePair {
            key: "link".into(),
            value: receiver_link.into(),
        }];
        let rendered = envelope(&request("application/json", Some(&pairs), None));

        let link = rendered["metadata"]["Link"].as_str().expect("a Link member");
        assert!(link.starts_with(&format!("<{}>", NGSI_LD_CORE_CONTEXT_URL.as_str())), "{link}");
        assert!(link.ends_with(&format!(", {receiver_link}")), "{link}");
        assert!(rendered["metadata"].get("link").is_none(), "{}", rendered["metadata"]);
    }
}
