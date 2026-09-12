//! One notification, ready to hand to a transport.

use crate::notification_context_placement::{NotificationContextPlacement, notification_context_placement};
use andromeda_http_headers::{link::header::Link, media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
use andromeda_ngsi_ld_lexicon::{positive_milliseconds::PositiveMilliseconds, tenant::TenantName, uri::Uri};
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
    /// Where clause 6.3.8 puts this notification's `@context`, decided once
    /// from the media type and the subscription's `Prefer` `receiverInfo` entry.
    pub context_placement: NotificationContextPlacement,
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
    /// The endpoint's own delivery deadline (Table 5.2.15-1 `timeout`), when it
    /// named one.
    ///
    /// Only a binding whose protocol "always returns a response" honours it,
    /// which is the HTTP binding; every other transport ignores it and keeps
    /// its own bound.
    pub timeout: Option<PositiveMilliseconds>,
    /// Subscription identifier, for logging and stream routing.
    pub subscription_id: Uri,
}

impl DeliveryRequest {
    /// The `Link` header this notification carries its `@context` in, if any.
    ///
    /// Clause 6.3.8 decides this per notification, not per media type alone:
    /// `application/json` always uses the header, `application/ld+json` always
    /// uses the body, and `application/geo+json` uses the body unless the
    /// subscription's `receiverInfo` asks for `Prefer: body=json`. Every
    /// transport asks this rather than deciding for itself, because three
    /// transports deciding independently is how `application/geo+json` came to
    /// carry no `@context` at all in either place.
    pub fn context_link(&self) -> Option<Link> {
        if self.context_placement == NotificationContextPlacement::Body {
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
    /// The subscription's `notification.endpoint.timeout`, if it set one.
    pub timeout: Option<PositiveMilliseconds>,
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
    /// Clause 6.3.6: when the body's media type is `application/ld+json` "the
    /// response payload body provided by the HTTP response shall include a
    /// JSON-LD @context". The other media types carry it in the `Link` header
    /// [`DeliveryRequest::context_link`] builds instead, and clause 6.3.5
    /// forbids mixing the two.
    ///
    /// The body is written in two places because it is two documents. The
    /// Notification itself is one -- Table 5.3.1-1 gives it an `id` that is the
    /// JSON-LD `@id` and a `type` that is the `@type` -- and every Entity in
    /// its `data` is another, since clause 4.5.1 gives an Entity the same two.
    /// A subscriber that lifts one Entity out of `data` holds a complete
    /// NGSI-LD document, exactly as it would from an entity query, where an
    /// array body leaves each Entity to carry its own. Both are written with
    /// the one `@context` clause 5.8.6 names, "the one specified in the
    /// jsonldContext field", so the inner one restates the outer rather than
    /// narrowing it.
    ///
    /// Infallible: serialisation, the only step that could fail, now happens in
    /// each transport against the body shape it sends.
    pub fn assemble(&self) -> DeliveryRequest {
        let content_type = notification_media_type(self.accept);
        let context_placement = notification_context_placement(content_type, self.receiver_info);
        // clone: the request owns the notification body the borrow points at, and the `Arc` lets each transport read it without another copy
        let mut payload = self.payload.clone();
        if context_placement == NotificationContextPlacement::Body
            && let Some(document) = payload.as_object_mut()
        {
            let context = self.jsonld_context.map_or(NGSI_LD_CORE_CONTEXT_URL.as_str(), Url::as_str);
            document.insert("@context".to_owned(), Value::String(context.to_owned()));
            for notified in document.get_mut("data").and_then(Value::as_array_mut).into_iter().flatten() {
                if let Some(notified) = notified.as_object_mut() {
                    notified.insert("@context".to_owned(), Value::String(context.to_owned()));
                }
            }
        }
        DeliveryRequest {
            payload: Arc::new(payload),
            endpoint_uri: self.endpoint_uri.clone(), // clone: the request outlives this borrow of the subscription
            content_type,
            context_placement,
            context_url: self.jsonld_context.cloned(), // clone: same
            // clone: `NgsiLdTenant` owns its name and the request outlives this borrow
            tenant: self.tenant.map(|name| NgsiLdTenant::new(name.clone())),
            // clone: `NgsiLdSnapshot` owns its URI and the request outlives this borrow
            snapshot: self.snapshot.map(|id| NgsiLdSnapshot::new(id.clone())),
            receiver_info: self.receiver_info.unwrap_or_default().to_vec(),
            timeout: self.timeout,
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
    use andromeda_ngsi_ld_lexicon::{positive_milliseconds::PositiveMilliseconds, tenant::TenantName, uri::Uri};
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
            timeout: None,
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

    /// Clause 6.3.6 has the `application/ld+json` body carry a JSON-LD
    /// `@context`, and every Entity in `data` is a document of its own
    /// (clause 4.5.1 gives it `@id` and `@type`), so it carries the same one:
    /// a subscriber can lift one Entity out of `data` and still expand it.
    #[test]
    fn a_json_ld_notification_writes_the_context_on_every_notified_entity_too() {
        let endpoint = endpoint();
        let subscription = subscription();
        let context = Url::parse("https://example.com/ctx.jsonld").expect("a well-formed context URI");
        let accept = media("application/ld+json");
        let request = NotificationDelivery {
            payload: &json!({
                "id": "urn:ngsi-ld:Notification:1",
                "type": "Notification",
                "data": [{"id": "urn:ngsi-ld:Building:1", "type": "Building"}, {"id": "urn:ngsi-ld:Building:2", "type": "Building"}]
            }),
            endpoint_uri: &endpoint,
            accept: Some(&accept),
            receiver_info: None,
            jsonld_context: Some(&context),
            tenant: None,
            snapshot: None,
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble();

        pretty_assertions::assert_eq!(request.payload["@context"], json!("https://example.com/ctx.jsonld"));
        for notified in request.payload["data"].as_array().expect("the data array survives") {
            pretty_assertions::assert_eq!(notified["@context"], json!("https://example.com/ctx.jsonld"), "{notified}");
        }
    }

    /// Clause 6.3.8: an `application/json` notification carries the context in
    /// the `Link` header, so its body holds no `@context` member.
    #[test]
    fn a_json_notification_body_carries_no_context_member() {
        let request = assemble(Some(&media("application/json")), Some("https://example.com/ctx.jsonld"));
        assert!(
            request.payload.get("@context").is_none(),
            "a JSON body must not embed a context: {}",
            request.payload
        );
        assert!(assemble(None, None).payload.get("@context").is_none());
    }

    /// Clause 6.3.8: with `geo+json` and no `receiverInfo`, "the HTTP
    /// notification request includes an `@context` element in the payload
    /// body". This is the notification rule, and it is the opposite of the
    /// clause 6.3.7 rule for a `GeoJSON` *response*.
    #[test]
    fn a_geo_json_notification_body_carries_the_context_member_by_default() {
        let request = assemble(Some(&media("application/geo+json")), Some("https://example.com/ctx.jsonld"));

        pretty_assertions::assert_eq!(request.payload["@context"], json!("https://example.com/ctx.jsonld"));
    }

    #[test]
    fn a_json_notification_carries_the_core_context_link_when_the_subscription_named_none() {
        let link = assemble(Some(&media("application/json")), None).context_link().expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    /// Clause 6.3.8: a `GeoJSON` notification whose subscription asked for no
    /// `Prefer: body=json` carries its context in the body, so it emits no
    /// `Link` header.
    #[test]
    fn a_geo_json_notification_carries_no_context_link_by_default() {
        assert!(assemble(Some(&media("application/geo+json")), None).context_link().is_none());
    }

    /// Clause 6.3.8: `Prefer: body=json` in `receiverInfo` moves a `GeoJSON`
    /// notification's context out of the body and into the `Link` header.
    #[test]
    fn a_geo_json_notification_preferring_json_carries_a_context_link_and_no_body_member() {
        let pairs = vec![KeyValuePair {
            key: "Prefer".into(),
            value: "body=json".into(),
        }];
        let endpoint = endpoint();
        let subscription = subscription();
        let request = NotificationDelivery {
            payload: &json!({"id": "urn:ngsi-ld:Notification:1"}),
            endpoint_uri: &endpoint,
            accept: Some(&media("application/geo+json")),
            receiver_info: Some(&pairs),
            jsonld_context: None,
            tenant: None,
            snapshot: None,
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble();

        assert!(request.payload.get("@context").is_none(), "{}", request.payload);
        let rendered = request.context_link().expect("a Link").to_header_string().expect("the Link encodes");
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
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.receiver_info, pairs);
    }

    /// Table 5.2.15-1: the endpoint's `timeout` reaches the transport that can
    /// honour it, and an endpoint that named none carries no deadline of its
    /// own.
    #[test]
    fn an_endpoint_timeout_reaches_the_assembled_request() {
        let endpoint_url = endpoint();
        let subscription = subscription();
        let timeout = PositiveMilliseconds::try_from(750).expect("a positive millisecond count");
        let request = NotificationDelivery {
            payload: &json!({}),
            endpoint_uri: &endpoint_url,
            accept: None,
            receiver_info: None,
            jsonld_context: None,
            tenant: None,
            snapshot: None,
            timeout: Some(timeout),
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.timeout, Some(timeout));
        assert!(assemble(None, None).timeout.is_none());
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
            timeout: None,
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
            timeout: None,
            subscription_id: &subscription,
        }
        .assemble();
        pretty_assertions::assert_eq!(request.snapshot.as_ref().map(NgsiLdSnapshot::as_str), Some(snapshot.as_str()));
    }
}
