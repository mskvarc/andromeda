//! One notification, ready to hand to a transport.

use andromeda_http_headers::{link::header::Link, media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriRefBuf;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use std::sync::Arc;
use url::Url;

/// Everything a transport needs to deliver a single notification.
///
/// Assembled once by [`NotificationDelivery::assemble`] and then read-only: the
/// media type is already resolved and the `@context` decision already made. The
/// notification body travels as a structured [`Value`], not as bytes, so each
/// transport serialises it in the one shape it needs — the HTTP body once, the
/// MQTT `{metadata, body}` envelope once — rather than the assembler
/// serialising it and one transport parsing it straight back.
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
    /// The notification document, shared by every transport that delivers it.
    pub payload: Arc<Value>,
    /// Target endpoint URL, carried as a [`Url`] so no transport re-parses the
    /// string the model already validated.
    pub endpoint_uri: Url,
    /// The media type the body is serialised as.
    pub content_type: ResponseMediaType,
    /// The `@context` this notification advertises, when the subscription
    /// named one. [`context_link`](DeliveryRequest::context_link) supplies the
    /// core context when it did not.
    pub context_url: Option<Url>,
    /// Tenant the notification originates from (clause 6.3.14).
    pub tenant: Option<NgsiLdTenant>,
    /// Snapshot branch the notification originates from (clause 6.3.22).
    pub snapshot: Option<NgsiLdSnapshot>,
    /// Additional key-value pairs from the subscription's `receiverInfo`.
    pub receiver_info: Vec<KeyValuePair>,
    /// Subscription identifier, for logging and stream routing.
    pub subscription_id: Uri,
}

impl DeliveryRequest {
    /// The `Link` header this notification carries its `@context` in, if any.
    ///
    /// Clause 5.5.7: `application/ld+json` carries the `@context` inside the
    /// body, and the other two media types carry it in a `Link` header. Every
    /// transport asks this rather than deciding for itself, because three
    /// transports deciding independently is how `application/geo+json` came to
    /// carry no `@context` at all in either place.
    pub fn context_link(&self) -> Option<Link> {
        if self.content_type.include_context_in_body() {
            return None;
        }
        // The `@context` a notification advertises is an IRI reference, so the
        // Link header carries it as a typed [`IriRefBuf`] rather than a bare
        // string; a subscription that named no context falls back to the core
        // context. A URL and the core-context constant are both valid IRI
        // references, so the construction does not fail in practice.
        let context = self.context_url.as_ref().map_or(NGSI_LD_CORE_CONTEXT_URL.as_str(), Url::as_str);
        IriRefBuf::new(context).ok().map(Link::jsonld_context)
    }
}

/// The notification a subscription asked for, before it is serialised.
///
/// Named fields rather than a seven-argument function: four of those arguments
/// were `Option<&str>` or `&str`, and swapping two of them is a mistake no
/// signature could have caught.
#[derive(Debug)]
pub struct NotificationDelivery<'a> {
    /// The notification document.
    pub payload: &'a Value,
    /// The endpoint URI the subscription named, already parsed by the model.
    pub endpoint_uri: &'a Url,
    /// The subscription's `notification.endpoint.accept`, if it set one.
    pub accept: Option<&'a MediaTypeBuf>,
    /// The subscription's `notification.endpoint.receiverInfo`, if it set any.
    pub receiver_info: Option<&'a [KeyValuePair]>,
    /// The subscription's `jsonldContext`, if it set one.
    pub jsonld_context: Option<&'a Url>,
    /// The tenant the subscription belongs to.
    pub tenant: Option<&'a TenantName>,
    /// The Snapshot branch the subscription belongs to, when scoped.
    pub snapshot: Option<&'a Uri>,
    /// The subscription's identifier.
    pub subscription_id: &'a Uri,
}

impl NotificationDelivery<'_> {
    /// Resolve everything a transport reads, carrying the notification body
    /// through as a shared [`Value`] for each transport to serialise once.
    ///
    /// Clauses 5.2.13 and 6.3.5: an `application/ld+json` body is a JSON-LD
    /// document and carries its `@context` itself, once, at the top level of
    /// the notification; the entities in `data` are plain members of that
    /// document and carry none of their own. The other media types carry the
    /// context in the `Link` header [`DeliveryRequest::context_link`] builds.
    ///
    /// Infallible: serialisation, the only step that could fail, now happens in
    /// each transport against the body shape it sends.
    pub fn assemble(&self) -> DeliveryRequest {
        let content_type = notification_media_type(self.accept);
        // clone: the request owns the notification body the borrow points at, and the `Arc` lets each transport read it without another copy
        let mut payload = self.payload.clone();
        if content_type.include_context_in_body()
            && let Some(document) = payload.as_object_mut()
        {
            let context = self.jsonld_context.map_or(NGSI_LD_CORE_CONTEXT_URL.as_str(), Url::as_str);
            document.insert("@context".to_owned(), Value::String(context.to_owned()));
        }
        DeliveryRequest {
            payload: Arc::new(payload),
            endpoint_uri: self.endpoint_uri.clone(), // clone: the request outlives this borrow of the subscription
            content_type,
            context_url: self.jsonld_context.cloned(), // clone: same
            // clone: `NgsiLdTenant` owns its name and the request outlives this borrow
            tenant: self.tenant.map(|name| NgsiLdTenant::new(name.clone())),
            // clone: `NgsiLdSnapshot` owns its URI and the request outlives this borrow
            snapshot: self.snapshot.map(|id| NgsiLdSnapshot::new(id.clone())),
            receiver_info: self.receiver_info.unwrap_or_default().to_vec(),
            subscription_id: self.subscription_id.clone(), // clone: the request outlives this borrow of the subscription
        }
    }
}

/// The media type a notification body is serialised as.
///
/// Clause 5.2.15 leaves `accept` optional and clause 6.3.4 makes
/// `application/json` the default; an `accept` naming something this broker
/// does not render falls back to the same default rather than failing the
/// delivery, because the subscription was accepted with that value and
/// refusing to notify would be a worse answer than notifying in JSON.
///
/// Public because the notification builder asks the same question one step
/// earlier — whether to leave the `@context` in the document it is compacting —
/// and that has to be the same answer this gives, not a second string
/// comparison against `application/ld+json`.
#[must_use]
pub fn notification_media_type(accept: Option<&MediaTypeBuf>) -> ResponseMediaType {
    accept
        .and_then(|media_type| ResponseMediaType::from_media_type(&media_type.to_ref()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::request::{DeliveryRequest, NotificationDelivery, notification_media_type};
    use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
    use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use mediatype::MediaTypeBuf;
    use serde_json::json;
    use std::str::FromStr;
    use url::Url;

    fn media(raw: &str) -> MediaTypeBuf {
        MediaTypeBuf::from_str(raw).expect("a well-formed media type")
    }

    fn endpoint() -> Url {
        Url::parse("https://receiver.example.com/notify").expect("a well-formed endpoint URI")
    }

    fn subscription() -> Uri {
        Uri::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id")
    }

    fn assemble(accept: Option<&MediaTypeBuf>, jsonld_context: Option<&str>) -> DeliveryRequest {
        let endpoint = endpoint();
        let context = jsonld_context.map(|value| Url::parse(value).expect("a well-formed context URI"));
        let subscription = subscription();
        NotificationDelivery {
            payload: &json!({"id": "urn:ngsi-ld:Notification:1"}),
            endpoint_uri: &endpoint,
            accept,
            receiver_info: None,
            jsonld_context: context.as_ref(),
            tenant: None,
            snapshot: None,
            subscription_id: &subscription,
        }
        .assemble()
    }

    #[test]
    fn a_subscription_that_named_no_accept_is_notified_in_json() {
        pretty_assertions::assert_eq!(notification_media_type(None), ResponseMediaType::Json);
    }

    #[test]
    fn each_media_type_clause_6_3_4_defines_is_recognised_from_the_accept_member() {
        pretty_assertions::assert_eq!(notification_media_type(Some(&media("application/json"))), ResponseMediaType::Json);
        pretty_assertions::assert_eq!(notification_media_type(Some(&media("application/ld+json"))), ResponseMediaType::JsonLd);
        pretty_assertions::assert_eq!(notification_media_type(Some(&media("application/geo+json"))), ResponseMediaType::GeoJson);
    }

    #[test]
    fn a_parameter_on_the_accept_member_does_not_stop_it_being_recognised() {
        pretty_assertions::assert_eq!(
            notification_media_type(Some(&media("application/ld+json; charset=utf-8"))),
            ResponseMediaType::JsonLd
        );
    }

    #[test]
    fn an_accept_this_broker_does_not_render_falls_back_to_json_rather_than_failing() {
        pretty_assertions::assert_eq!(notification_media_type(Some(&media("application/xml"))), ResponseMediaType::Json);
    }

    #[test]
    fn a_json_ld_notification_carries_no_link_because_the_context_is_in_the_body() {
        assert!(assemble(Some(&media("application/ld+json")), None).context_link().is_none());
    }

    /// Clauses 5.2.13 and 6.3.5: the JSON-LD body carries one top-level
    /// `@context`, the subscription's own when it named one and the core
    /// context otherwise.
    #[test]
    fn a_json_ld_notification_carries_one_top_level_context_in_the_body() {
        let core = assemble(Some(&media("application/ld+json")), None);
        pretty_assertions::assert_eq!(core.payload["@context"], json!(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        pretty_assertions::assert_eq!(core.payload["id"], json!("urn:ngsi-ld:Notification:1"));

        let named = assemble(Some(&media("application/ld+json")), Some("https://example.com/ctx.jsonld"));
        pretty_assertions::assert_eq!(named.payload["@context"], json!("https://example.com/ctx.jsonld"));
    }

    /// The other media types carry the context in the `Link` header and never
    /// in the body.
    #[test]
    fn a_json_or_geo_json_notification_body_carries_no_context_member() {
        for accept in ["application/json", "application/geo+json"] {
            let request = assemble(Some(&media(accept)), Some("https://example.com/ctx.jsonld"));
            assert!(
                request.payload.get("@context").is_none(),
                "{accept} must not embed a context: {}",
                request.payload
            );
        }
        assert!(assemble(None, None).payload.get("@context").is_none());
    }

    #[test]
    fn a_json_notification_carries_the_core_context_link_when_the_subscription_named_none() {
        let link = assemble(Some(&media("application/json")), None).context_link().expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    #[test]
    fn a_geo_json_notification_carries_a_context_link_too_because_clause_5_5_7_makes_no_exception() {
        let link = assemble(Some(&media("application/geo+json")), None).context_link().expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    #[test]
    fn the_subscriptions_own_context_wins_over_the_core_context() {
        let link = assemble(Some(&media("application/json")), Some("https://example.com/ctx.jsonld"))
            .context_link()
            .expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains("https://example.com/ctx.jsonld"), "{rendered}");
        assert!(!rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    #[test]
    fn receiver_info_reaches_the_request_and_an_absent_one_is_an_empty_list() {
        pretty_assertions::assert_eq!(assemble(None, None).receiver_info, Vec::new());

        let pairs = vec![KeyValuePair {
            key: "Authorization".into(),
            value: "Bearer token".into(),
        }];
        let endpoint = endpoint();
        let subscription = subscription();
        let request = NotificationDelivery {
            payload: &json!({}),
            endpoint_uri: &endpoint,
            accept: None,
            receiver_info: Some(&pairs),
            jsonld_context: None,
            tenant: None,
            snapshot: None,
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.receiver_info, pairs);
    }

    #[test]
    fn the_tenant_reaches_the_request_as_the_header_that_carries_it() {
        let name = TenantName::try_from("acme".to_string()).expect("a valid tenant name");
        let endpoint = endpoint();
        let subscription = subscription();
        let request = NotificationDelivery {
            payload: &json!({}),
            endpoint_uri: &endpoint,
            accept: None,
            receiver_info: None,
            jsonld_context: None,
            tenant: Some(&name),
            snapshot: None,
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.tenant.as_ref().map(NgsiLdTenant::as_str), Some("acme"));
    }

    #[test]
    fn the_snapshot_reaches_the_request_as_the_header_that_carries_it() {
        let snapshot = Uri::from_str("urn:ngsi-ld:Snapshot:branch").expect("a valid Snapshot id");
        let endpoint = endpoint();
        let subscription = subscription();
        let request = NotificationDelivery {
            payload: &json!({}),
            endpoint_uri: &endpoint,
            accept: None,
            receiver_info: None,
            jsonld_context: None,
            tenant: None,
            snapshot: Some(&snapshot),
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.snapshot.as_ref().map(NgsiLdSnapshot::as_str), Some(snapshot.as_str()));
    }
}
