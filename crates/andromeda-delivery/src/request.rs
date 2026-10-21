//! One notification, ready to hand to a transport.

use crate::{
    notification_body::{NotificationBody, NotificationWriteError},
    notification_context::NotificationContext,
    notification_context_placement::{NotificationContextPlacement, notification_context_placement},
};
use andromeda_http_headers::{link::header::Link, media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId, positive_milliseconds::PositiveMilliseconds, tenant::TenantName};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    notification::{Notification, NotificationData},
};
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use chrono::Utc;
use iri_rs::IriBuf;
use mediatype::MediaTypeBuf;
use typed_builder::TypedBuilder;
use url::Url;

/// Everything a transport needs to deliver a single notification.
///
/// Assembled once by [`NotificationDelivery::assemble`] and then read-only: the
/// media type is already resolved, the `@context` decision already made and
/// the notification already written, so each transport sends the one text in
/// the shape it needs (the HTTP body as it is, the MQTT `{metadata, body}`
/// envelope around it) and none writes the document again.
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
    /// The written notification, shared by every transport that delivers it.
    pub body: NotificationBody,
    /// Target endpoint URL, carried as a [`Url`] so no transport re-parses the
    /// string the model already validated.
    pub endpoint_uri: Url,
    /// The media type the body is written as.
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
    /// Only a binding whose protocol "always returns a response" honours it:
    /// the HTTP binding, and an MQTT publish at a `QoS` the broker acknowledges.
    /// Every other transport ignores it and keeps its own bound.
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
    /// transport asks this rather than deciding for itself, because transports
    /// deciding independently could leave an `application/geo+json` notification
    /// with no `@context` in either place.
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

/// The delivery a subscription asked for, before its notification is written.
///
/// Built through its builder rather than a positional function: four of the
/// references it takes are optional members of the subscription, and swapping
/// two of them is a mistake no signature could have caught.
#[derive(Debug, TypedBuilder)]
pub struct NotificationDelivery<'a> {
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

impl<'a> NotificationDelivery<'a> {
    /// The media type the notification is written and sent as.
    #[must_use]
    pub fn content_type(&self) -> ResponseMediaType {
        notification_media_type(self.accept)
    }

    /// Where clause 6.3.8 puts the notification's `@context`.
    #[must_use]
    pub fn context_placement(&self) -> NotificationContextPlacement {
        notification_context_placement(self.content_type(), self.receiver_info)
    }

    /// The `@context` the notification body carries, when clause 6.3.8 puts it
    /// there.
    ///
    /// Clause 6.3.6: when the body's media type is `application/ld+json` "the
    /// response payload body provided by the HTTP response shall include a
    /// JSON-LD @context", and an `application/geo+json` notification carries it
    /// in the body too unless its `receiverInfo` prefers `body=json` (clause
    /// 6.3.8). The other notifications carry it in the `Link` header
    /// [`DeliveryRequest::context_link`] builds instead, and clause 6.3.5
    /// forbids mixing the two. The writer of the notification puts it on the
    /// Notification and wherever inside `data` the shape of `data` asks: clause
    /// 5.8.6 names "the one specified in the jsonldContext field", with the
    /// core context clause 5.2.3 adds to it.
    #[must_use]
    pub fn body_context(&self) -> Option<&'a JsonLiteral> {
        match self.context_placement() {
            NotificationContextPlacement::Body => Some(self.context.body_context()),
            NotificationContextPlacement::LinkHeader => None,
        }
    }

    /// The clause 5.3.1 Notification carrying `data`, written for this
    /// delivery, and everything a transport reads to send it.
    ///
    /// The Notification carries the body's `@context` when clause 6.3.8 puts it
    /// there; `data` was written for the same delivery, so it carries it
    /// wherever its shape asks.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationWriteError`] when the notification identifier
    /// cannot be minted or the notification cannot be written.
    pub fn notification(&self, data: NotificationData) -> Result<DeliveryRequest, NotificationWriteError> {
        let id = Identifier::from(NotificationId::mint()?);
        let notification = Notification::builder()
            .context(self.body_context())
            .id(&id)
            .subscription_id(self.subscription_id)
            .notified_at(Utc::now())
            .data(data)
            .build();
        Ok(self.assemble(NotificationBody::written(&id, &notification)?))
    }

    /// Everything a transport reads to send `body`, the notification written
    /// for this delivery.
    #[must_use]
    pub fn assemble(&self, body: NotificationBody) -> DeliveryRequest {
        DeliveryRequest {
            body,
            endpoint_uri: self.endpoint_uri.clone(), // clone: the request outlives this borrow of the subscription
            content_type: self.content_type(),
            context_placement: self.context_placement(),
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

/// The media type a notification body is written as.
///
/// Clause 5.2.15 leaves `accept` optional and clause 6.3.4 makes
/// `application/json` the default; an `accept` naming something this broker
/// does not render falls back to the same default rather than failing the
/// delivery, because the subscription was accepted with that value and
/// refusing to notify would be a worse answer than notifying in JSON.
fn notification_media_type(accept: Option<&MediaTypeBuf>) -> ResponseMediaType {
    accept
        .and_then(|media_type| ResponseMediaType::from_media_type(&media_type.to_ref()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        notification_body::NotificationBody,
        notification_context::NotificationContext,
        request::{DeliveryRequest, NotificationDelivery, notification_media_type},
    };
    use andromeda_http_headers::{media_type::ResponseMediaType, ngsild_snapshot::NgsiLdSnapshot, ngsild_tenant::NgsiLdTenant};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, positive_milliseconds::PositiveMilliseconds, tenant::TenantName, time::date_time::NgsiLdDateTime};
    use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, notification::NotificationData};
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use mediatype::MediaTypeBuf;
    use serde_json::{json, value::RawValue};
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
                JsonLiteral::admit(&json!([url, NGSI_LD_CORE_CONTEXT_URL.as_str()]).to_string()).expect("a context"),
            ),
            None => NotificationContext::stated(
                None,
                JsonLiteral::admit(&json!(NGSI_LD_CORE_CONTEXT_URL.as_str()).to_string()).expect("a context"),
            ),
        }
    }

    fn body() -> NotificationBody {
        let id = Identifier::from_str("urn:ngsi-ld:Notification:1").expect("a well-formed notification id");
        NotificationBody::written(&id, &json!({"id": "urn:ngsi-ld:Notification:1"})).expect("a written notification")
    }

    /// Whether the delivery of an endpoint accepting `accept`, with
    /// `receiver_info`, carries the context in the body, and the request it
    /// assembles.
    fn delivered(accept: Option<&MediaTypeBuf>, receiver_info: Option<&[KeyValuePair]>, jsonld_context: Option<&str>) -> (bool, DeliveryRequest) {
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(jsonld_context);
        let delivery = NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .accept(accept)
            .receiver_info(receiver_info)
            .context(&context)
            .subscription_id(&subscription)
            .build();
        let in_body = delivery.body_context().is_some_and(|carried| carried == context.body_context());
        (in_body, delivery.assemble(body()))
    }

    fn prefer_json() -> Vec<KeyValuePair> {
        vec![KeyValuePair {
            key: "Prefer".into(),
            value: "body=json".into(),
        }]
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

    /// Clauses 5.2.3, 6.3.5 and 6.3.6: a JSON-LD notification carries the
    /// context the subscription's processor resolved in its body, and no
    /// `Link`.
    #[test]
    fn a_json_ld_notification_carries_its_context_in_the_body_and_no_link() {
        let (in_body, request) = delivered(Some(&media("application/ld+json")), None, Some("https://example.com/ctx.jsonld"));
        assert!(in_body);
        assert!(request.context_link().is_none());
        pretty_assertions::assert_eq!(request.content_type, ResponseMediaType::JsonLd);
    }

    /// Clause 6.3.8: an `application/json` notification carries the context in
    /// the `Link` header, so its body holds no `@context` member.
    #[test]
    fn a_json_notification_carries_no_body_context() {
        let (in_body, request) = delivered(Some(&media("application/json")), None, Some("https://example.com/ctx.jsonld"));
        assert!(!in_body);
        assert!(request.context_link().is_some());
        assert!(!delivered(None, None, None).0);
    }

    /// Clause 6.3.8: with `geo+json` and no `receiverInfo`, "the HTTP
    /// notification request includes an `@context` element in the payload
    /// body". This is the notification rule, and it is the opposite of the
    /// clause 6.3.7 rule for a `GeoJSON` *response*.
    #[test]
    fn a_geo_json_notification_carries_its_context_in_the_body_by_default() {
        let (in_body, request) = delivered(Some(&media("application/geo+json")), None, Some("https://example.com/ctx.jsonld"));
        assert!(in_body);
        assert!(request.context_link().is_none());
    }

    /// Clause 6.3.8 and Table 5.3.1-1: `Prefer: body=json` in `receiverInfo`
    /// moves a `GeoJSON` notification's context out of the body and into the
    /// `Link` header.
    #[test]
    fn a_geo_json_notification_preferring_json_carries_a_context_link_and_no_body_context() {
        let pairs = prefer_json();
        let (in_body, request) = delivered(Some(&media("application/geo+json")), Some(&pairs), None);
        assert!(!in_body);
        let rendered = request.context_link().expect("a Link").to_header_string().expect("the Link encodes");
        assert!(rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    #[test]
    fn a_json_notification_carries_the_core_context_link_when_the_subscription_named_none() {
        let link = delivered(Some(&media("application/json")), None, None).1.context_link().expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    #[test]
    fn the_subscriptions_own_context_wins_over_the_core_context() {
        let link = delivered(Some(&media("application/json")), None, Some("https://example.com/ctx.jsonld"))
            .1
            .context_link()
            .expect("a Link");
        let rendered = link.to_header_string().expect("the Link encodes");
        assert!(rendered.contains("https://example.com/ctx.jsonld"), "{rendered}");
        assert!(!rendered.contains(NGSI_LD_CORE_CONTEXT_URL.as_str()), "{rendered}");
    }

    /// The notification of an endpoint accepting `accept`, written around
    /// `data_text`, a `data` member the sender wrote once.
    fn enveloped(accept: &str, data_text: &str) -> DeliveryRequest {
        let endpoint = endpoint();
        let subscription = subscription();
        let accept = media(accept);
        let context = context_of(Some("https://example.com/ctx.jsonld"));
        let data = NotificationData::written(RawValue::from_string(data_text.to_owned()).expect("a JSON data member"));
        NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .accept(Some(&accept))
            .context(&context)
            .subscription_id(&subscription)
            .build()
            .notification(data)
            .expect("the notification is written")
    }

    /// Clause 5.3.1: the `data` the sender wrote reaches the body as that
    /// text and nowhere else, so an entity is in the notification once.
    #[test]
    fn an_entity_is_written_once_per_notification() {
        let data = r#"[{"id":"urn:ngsi-ld:Building:1","type":"Building"},{"id":"urn:ngsi-ld:Building:2","type":"Building"}]"#;
        let request = enveloped("application/json", data);
        let text = request.body.text();
        pretty_assertions::assert_eq!(text.matches(data).count(), 1, "{text}");
        for entity in ["urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2"] {
            pretty_assertions::assert_eq!(text.matches(entity).count(), 1, "{text}");
        }
    }

    /// Clause 6.3.6: an `application/ld+json` notification names its context
    /// in its body, on the Notification and on every Entity of `data`, which
    /// the sender wrote under that context; an `application/json` one names it
    /// nowhere in the body (clause 6.3.8 puts it in the `Link` header).
    #[test]
    fn ld_json_entities_carry_their_context() {
        let data = r#"[{"@context":"https://example.com/ctx.jsonld","id":"urn:ngsi-ld:Building:1","type":"Building"}]"#;
        let json_ld: serde_json::Value = serde_json::from_str(enveloped("application/ld+json", data).body.text()).expect("a JSON body");
        let expected = json!(["https://example.com/ctx.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()]);
        pretty_assertions::assert_eq!(json_ld["@context"], expected);
        pretty_assertions::assert_eq!(json_ld["data"][0]["@context"], json!("https://example.com/ctx.jsonld"));

        let plain: serde_json::Value = serde_json::from_str(
            enveloped("application/json", r#"[{"id":"urn:ngsi-ld:Building:1","type":"Building"}]"#)
                .body
                .text(),
        )
        .expect("a JSON body");
        assert!(plain.get("@context").is_none(), "{plain}");
        assert!(plain["data"][0].get("@context").is_none(), "{plain}");
    }

    /// The written notification reaches the request as it was written.
    #[test]
    fn the_written_notification_reaches_the_request_unchanged() {
        let request = delivered(None, None, None).1;
        pretty_assertions::assert_eq!(request.body.text(), r#"{"id":"urn:ngsi-ld:Notification:1"}"#);
        pretty_assertions::assert_eq!(request.body.id().as_str(), "urn:ngsi-ld:Notification:1");
    }

    #[test]
    fn receiver_info_reaches_the_request_and_an_absent_one_is_an_empty_list() {
        pretty_assertions::assert_eq!(delivered(None, None, None).1.receiver_info, Vec::new());

        let pairs = vec![KeyValuePair {
            key: "Authorization".into(),
            value: "Bearer token".into(),
        }];
        pretty_assertions::assert_eq!(delivered(None, Some(&pairs), None).1.receiver_info, pairs);
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
            .endpoint_uri(&endpoint_url)
            .context(&context)
            .timeout(Some(timeout))
            .subscription_id(&subscription)
            .build()
            .assemble(body());
        pretty_assertions::assert_eq!(request.timeout, Some(timeout));
        assert!(delivered(None, None, None).1.timeout.is_none());
    }

    /// Clause 4.6.3 caps a `DateTime` at six fractional digits, and the broker
    /// reads a received `notifiedAt` under that cap, so the notification it
    /// stamps from its own clock is one it would accept.
    #[test]
    fn a_notification_is_stamped_with_an_admissible_notified_at() {
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .context(&context)
            .subscription_id(&subscription)
            .build()
            .notification(NotificationData::written(RawValue::from_string("[]".to_owned()).expect("written JSON")))
            .expect("a written notification");
        let written: serde_json::Value = serde_json::from_str(request.body.text()).expect("the notification is JSON");
        let notified_at = written["notifiedAt"].as_str().expect("notifiedAt is a string");
        assert!(notified_at.parse::<NgsiLdDateTime>().is_ok(), "{notified_at}");
    }

    #[test]
    fn the_tenant_reaches_the_request_as_the_header_that_carries_it() {
        let name = TenantName::try_from("acme".to_string()).expect("a valid tenant name");
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .context(&context)
            .tenant(Some(&name))
            .subscription_id(&subscription)
            .build()
            .assemble(body());
        pretty_assertions::assert_eq!(request.tenant.as_ref().map(NgsiLdTenant::as_str), Some("acme"));
    }

    #[test]
    fn the_snapshot_reaches_the_request_as_the_header_that_carries_it() {
        let snapshot = Identifier::from_str("urn:ngsi-ld:Snapshot:branch").expect("a valid Snapshot id");
        let endpoint = endpoint();
        let subscription = subscription();
        let context = context_of(None);
        let request = NotificationDelivery::builder()
            .endpoint_uri(&endpoint)
            .context(&context)
            .snapshot(Some(&snapshot))
            .subscription_id(&subscription)
            .build()
            .assemble(body());
        pretty_assertions::assert_eq!(request.snapshot.as_ref().map(NgsiLdSnapshot::as_str), Some(snapshot.as_str()));
    }
}
