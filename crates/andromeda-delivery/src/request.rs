//! One notification, ready to hand to a transport.

use crate::{
    notification_context::NotificationContext,
    notification_context_placement::{NotificationContextPlacement, notification_context_placement},
};
use andromeda_http_headers::{link::header::Link, media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, positive_milliseconds::PositiveMilliseconds, tenant::TenantName};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriBuf;
use mediatype::MediaTypeBuf;
use serde_json::{Map, Value};
use std::sync::Arc;
use typed_builder::TypedBuilder;
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
    pub context_url: Option<IriBuf>,
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
    pub subscription_id: Identifier,
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
    #[must_use]
    pub fn context_link(&self) -> Option<Link> {
        match self.context_placement {
            NotificationContextPlacement::Body => None,
            // A subscription that named no context is notified under the core
            // context (clause 5.5.5).
            NotificationContextPlacement::LinkHeader => Some(Link::jsonld_context(
                self.context_url.clone().unwrap_or_else(|| IriBuf::from(NGSI_LD_CORE_CONTEXT_URL)), // clone: the request keeps its context for another delivery attempt
            )),
        }
    }
}

/// The notification a subscription asked for, before it is serialised.
///
/// Built through its builder rather than a positional function: four of the
/// references it takes are optional members of the subscription, and swapping
/// two of them is a mistake no signature could have caught.
#[derive(Debug, TypedBuilder)]
pub struct NotificationDelivery<'a> {
    /// The notification document.
    payload: &'a Value,
    /// The endpoint URI the subscription named, already parsed by the model.
    endpoint_uri: &'a Url,
    /// The subscription's `notification.endpoint.accept`, if it set one.
    #[builder(default = None)]
    accept: Option<&'a MediaTypeBuf>,
    /// The subscription's `notification.endpoint.receiverInfo`, if it set any.
    #[builder(default = None)]
    receiver_info: Option<&'a [KeyValuePair]>,
    /// The `@context` the notification carries, resolved once for both the
    /// `Link` header and the body.
    context: &'a NotificationContext,
    /// The subscription's `notification.endpoint.timeout`, if it set one.
    #[builder(default = None)]
    timeout: Option<PositiveMilliseconds>,
    /// The tenant the subscription belongs to.
    #[builder(default = None)]
    tenant: Option<&'a TenantName>,
    /// The Snapshot branch the subscription belongs to, when scoped.
    #[builder(default = None)]
    snapshot: Option<&'a Identifier>,
    /// The subscription's identifier.
    subscription_id: &'a Identifier,
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
    /// jsonldContext field" with the core context clause 5.2.3 adds to it, so
    /// the inner one restates the outer rather than narrowing it.
    ///
    /// An `application/geo+json` notification's `data` is one `FeatureCollection`
    /// rather than an array (Table 5.3.1-1), so that `FeatureCollection` carries
    /// the context instead, and no Feature does (clauses 4.5.16.3 and
    /// 4.5.17.2: "separate @context elements for each Feature will not be
    /// present"). With `Prefer: body=json` in `receiverInfo` "the
    /// `FeatureCollection` will not contain an @context field" (Table 5.3.1-1)
    /// and the `Link` header names the context.
    ///
    /// Infallible: serialisation, the only step that could fail, now happens in
    /// each transport against the body shape it sends.
    #[must_use]
    pub fn assemble(&self) -> DeliveryRequest {
        let content_type = notification_media_type(self.accept);
        let context_placement = notification_context_placement(content_type, self.receiver_info);
        // clone: the request owns the notification body the borrow points at, and the `Arc` lets each transport read it without another copy
        let mut payload = self.payload.clone();
        if let Some(document) = payload.as_object_mut() {
            place_context(document, context_placement, self.context.body_context());
        }
        DeliveryRequest {
            payload: Arc::new(payload),
            endpoint_uri: self.endpoint_uri.clone(), // clone: the request outlives this borrow of the subscription
            content_type,
            context_placement,
            context_url: self.context.jsonld_context().cloned(), // clone: the request outlives this borrow of the subscription
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

/// Write the one notification `@context` wherever `placement` puts it: on the
/// envelope and on each notified Entity, or on the envelope and on the `data`
/// `FeatureCollection`; and nowhere in a body whose context travels in the
/// `Link` header.
fn place_context(document: &mut Map<String, Value>, placement: NotificationContextPlacement, body_context: &Value) {
    match placement {
        NotificationContextPlacement::Body => {
            // clone: the envelope and every document inside it each own a copy of the one context
            document.insert("@context".to_owned(), body_context.clone());
            match document.get_mut("data") {
                Some(Value::Array(entities)) => {
                    for entity in entities.iter_mut().filter_map(Value::as_object_mut) {
                        entity.insert("@context".to_owned(), body_context.clone()); // clone: as above
                    }
                }
                Some(Value::Object(collection)) => {
                    collection.insert("@context".to_owned(), body_context.clone()); // clone: as above
                    strip_feature_contexts(collection);
                }
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) | None => {}
            }
        }
        NotificationContextPlacement::LinkHeader => {
            if let Some(Value::Object(collection)) = document.get_mut("data") {
                collection.remove("@context");
                strip_feature_contexts(collection);
            }
        }
    }
}

/// Remove the `@context` of every Feature of a `FeatureCollection`.
fn strip_feature_contexts(collection: &mut Map<String, Value>) {
    for feature in collection.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(feature) = feature.as_object_mut() {
            feature.remove("@context");
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
    use crate::{
        notification_context::NotificationContext,
        request::{DeliveryRequest, NotificationDelivery, notification_media_type},
    };
    use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, positive_milliseconds::PositiveMilliseconds, tenant::TenantName};
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
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

    fn subscription() -> Identifier {
        Identifier::from_str("urn:ngsi-ld:Subscription:1").expect("a well-formed subscription id")
    }

    /// The notification `@context` of a subscription whose `jsonldContext` is
    /// `jsonld_context`, when the named context is not known to end with the
    /// core context.
    fn context_of(jsonld_context: Option<&str>) -> NotificationContext {
        match jsonld_context {
            Some(url) => NotificationContext::stated(
                Some(IriBuf::new(url.to_owned()).expect("a well-formed context URI")),
                json!([url, NGSI_LD_CORE_CONTEXT_URL.as_str()]),
            ),
            None => NotificationContext::stated(None, json!(NGSI_LD_CORE_CONTEXT_URL.as_str())),
        }
    }

    fn assemble(accept: Option<&MediaTypeBuf>, jsonld_context: Option<&str>) -> DeliveryRequest {
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(jsonld_context);
        NotificationDelivery::builder()
            .payload(&json!({"id": "urn:ngsi-ld:Notification:1"}))
            .endpoint_uri(&endpoint)
            .accept(accept)
            .context(&context)
            .subscription_id(&subscription)
            .build()
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

    /// Clauses 5.2.3 and 6.3.5: the JSON-LD body carries one top-level
    /// `@context`, the one the subscription's processor resolved.
    #[test]
    fn a_json_ld_notification_carries_one_top_level_context_in_the_body() {
        let core = assemble(Some(&media("application/ld+json")), None);
        pretty_assertions::assert_eq!(core.payload["@context"], json!(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        pretty_assertions::assert_eq!(core.payload["id"], json!("urn:ngsi-ld:Notification:1"));

        let named = assemble(Some(&media("application/ld+json")), Some("https://example.com/ctx.jsonld"));
        pretty_assertions::assert_eq!(
            named.payload["@context"],
            json!(["https://example.com/ctx.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()])
        );
    }

    /// Clause 6.3.6 has the `application/ld+json` body carry a JSON-LD
    /// `@context`, and every Entity in `data` is a document of its own
    /// (clause 4.5.1 gives it `@id` and `@type`), so it carries the same one:
    /// a subscriber can lift one Entity out of `data` and still expand it.
    #[test]
    fn a_json_ld_notification_writes_the_context_on_every_notified_entity_too() {
        let endpoint = endpoint();
        let subscription = subscription();
        let accept = media("application/ld+json");
        let context = context_of(Some("https://example.com/ctx.jsonld"));
        let request = NotificationDelivery::builder()
            .payload(&json!({
                "id": "urn:ngsi-ld:Notification:1",
                "type": "Notification",
                "data": [{"id": "urn:ngsi-ld:Building:1", "type": "Building"}, {"id": "urn:ngsi-ld:Building:2", "type": "Building"}]
            }))
            .endpoint_uri(&endpoint)
            .accept(Some(&accept))
            .context(&context)
            .subscription_id(&subscription)
            .build()
            .assemble();

        let expected = context.body_context();
        pretty_assertions::assert_eq!(&request.payload["@context"], expected);
        for notified in request.payload["data"].as_array().expect("the data array survives") {
            pretty_assertions::assert_eq!(&notified["@context"], expected, "{notified}");
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

        pretty_assertions::assert_eq!(&request.payload["@context"], context_of(Some("https://example.com/ctx.jsonld")).body_context());
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
        let accept = media("application/geo+json");
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .payload(&json!({"id": "urn:ngsi-ld:Notification:1"}))
            .endpoint_uri(&endpoint)
            .accept(Some(&accept))
            .receiver_info(Some(&pairs))
            .context(&context)
            .subscription_id(&subscription)
            .build()
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
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .payload(&json!({}))
            .endpoint_uri(&endpoint)
            .receiver_info(Some(&pairs))
            .context(&context)
            .subscription_id(&subscription)
            .build()
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
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .payload(&json!({}))
            .endpoint_uri(&endpoint_url)
            .context(&context)
            .timeout(Some(timeout))
            .subscription_id(&subscription)
            .build()
            .assemble();
        pretty_assertions::assert_eq!(request.timeout, Some(timeout));
        assert!(assemble(None, None).timeout.is_none());
    }

    #[test]
    fn the_tenant_reaches_the_request_as_the_header_that_carries_it() {
        let name = TenantName::try_from("acme".to_string()).expect("a valid tenant name");
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .payload(&json!({}))
            .endpoint_uri(&endpoint)
            .context(&context)
            .tenant(Some(&name))
            .subscription_id(&subscription)
            .build()
            .assemble();
        pretty_assertions::assert_eq!(request.tenant.as_ref().map(NgsiLdTenant::as_str), Some("acme"));
    }

    #[test]
    fn the_snapshot_reaches_the_request_as_the_header_that_carries_it() {
        let snapshot = Identifier::from_str("urn:ngsi-ld:Snapshot:branch").expect("a valid Snapshot id");
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .payload(&json!({}))
            .endpoint_uri(&endpoint)
            .context(&context)
            .snapshot(Some(&snapshot))
            .subscription_id(&subscription)
            .build()
            .assemble();
        pretty_assertions::assert_eq!(request.snapshot.as_ref().map(NgsiLdSnapshot::as_str), Some(snapshot.as_str()));
    }

    /// A `GeoJSON` notification the subscription asked for with `accept` and,
    /// optionally, `receiverInfo`, whose `data` is a `FeatureCollection` whose
    /// renderer wrote a context on it and on its Feature.
    fn geo_json_notification(receiver_info: Option<&[KeyValuePair]>) -> DeliveryRequest {
        let endpoint = endpoint();
        let subscription = subscription();
        let accept = media("application/geo+json");
        let context = context_of(None);
        NotificationDelivery::builder()
            .payload(&json!({
                "id": "urn:ngsi-ld:Notification:1",
                "type": "Notification",
                "data": {
                    "type": "FeatureCollection",
                    "@context": "https://renderer.example/stale.jsonld",
                    "features": [{ "id": "urn:ngsi-ld:Building:1", "type": "Feature", "@context": "https://renderer.example/stale.jsonld" }]
                }
            }))
            .endpoint_uri(&endpoint)
            .accept(Some(&accept))
            .receiver_info(receiver_info)
            .context(&context)
            .subscription_id(&subscription)
            .build()
            .assemble()
    }

    /// Table 5.3.1-1 and clause 4.5.16.3: a `GeoJSON` notification's context is
    /// on the envelope and on the `data` `FeatureCollection`, and on no Feature.
    #[test]
    fn a_geo_json_notification_carries_its_context_on_the_feature_collection_and_no_feature() {
        let request = geo_json_notification(None);

        let expected = context_of(None);
        pretty_assertions::assert_eq!(&request.payload["@context"], expected.body_context());
        pretty_assertions::assert_eq!(&request.payload["data"]["@context"], expected.body_context());
        for feature in request.payload["data"]["features"].as_array().expect("the features survive") {
            assert!(feature.get("@context").is_none(), "{feature}");
        }
        assert!(request.context_link().is_none());
    }

    /// Table 5.3.1-1: with `Prefer: body=json` "the `FeatureCollection` will not
    /// contain an @context field"; the `Link` header names the context.
    #[test]
    fn a_geo_json_notification_preferring_json_carries_no_context_on_the_feature_collection() {
        let pairs = vec![KeyValuePair {
            key: "Prefer".into(),
            value: "body=json".into(),
        }];
        let request = geo_json_notification(Some(&pairs));

        assert!(request.payload.get("@context").is_none(), "{}", request.payload);
        assert!(request.payload["data"].get("@context").is_none(), "{}", request.payload);
        for feature in request.payload["data"]["features"].as_array().expect("the features survive") {
            assert!(feature.get("@context").is_none(), "{feature}");
        }
        assert!(request.context_link().is_some());
    }
}
