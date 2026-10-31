//! The Context Source Registration Subscription operations of the conformance tests, and the reading of the
//! notification state a retrieved subscription reports.

use crate::{
    entity_operations::{expect_created, ld_json_body_client},
    geometry_payloads::point,
    subscription_payloads::{building_registration_subscription, key_values_document, key_values_notification},
};
use andromeda_test_suite::{
    broker::BrokerClient,
    cleanup::Cleanup,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        UncheckedRequest,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::Input,
    query::{EntitySelector, geoquery},
    subscription::{Activation, Endpoint, NotificationParams, RegistrationSubscription, Subscription, SubscriptionFragment, SubscriptionId},
};
use serde_json::Value;
use std::future::Future;

/// `Create Context Source Registration Subscription` of `subscription` with `document` as its body, sent by `client`
/// with the `@context` it carries: the request, unsent. The typed operation supplies the target and the headers.
pub(crate) fn registration_subscription_request(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    document: &UncheckedDocument,
) -> Result<UncheckedRequest> {
    Ok(client.unchecked(&CreateSubscription { subscription })?.document(document))
}

/// Creates `subscription` with its notification to `uri` in the keyValues spelling ([`key_values_document`]), sent by
/// `client` with the `@context` it carries, requires 201, and answers the cleanup that deletes the subscription.
pub(crate) async fn create_registration_subscription(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    uri: &url::Url,
) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).registration_subscription_of(subscription);
    let request = registration_subscription_request(client, subscription, &key_values_document(subscription, uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// `Create Context Source Registration Subscription` of `subscription` as [`create_registration_subscription`] sends
/// it: requires 201, the empty body, and a `Location` naming the subscription's id; answers the cleanup that deletes
/// the subscription.
pub(crate) async fn expect_registration_subscription_created(
    client: &BrokerClient,
    subscription: &Subscription<Input, RegistrationSubscription>,
    uri: &url::Url,
) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).registration_subscription_of(subscription);
    let request = registration_subscription_request(client, subscription, &key_values_document(subscription, uri)?)?;
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    expect_created(&answer?, subscription.id().or_fail()?);
    Ok(cleanup)
}

/// `Setup Initial Context Source Registration Subscriptions` of the query cases, first part: registers the terms
/// `Building`, `name` and `subCategory` with `tenant`, then in that context the Building subscriptions
/// `urn:ngsi-ld:Subscription:<ns>-1` and `-2` (the second watching `name` and `subCategory`), both notifying `uri`
/// (201 each); answers the vocabulary, the ids and the cleanup that deletes both subscriptions.
pub(crate) async fn create_registration_subscription_pair(
    tenant: &BrokerClient,
    namespace: &Namespace,
    uri: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 2], Cleanup)> {
    let vocabulary = Vocabulary::register(tenant, namespace, &["Building", "name", "subCategory"]).await?;
    let client = ld_json_body_client(tenant, &vocabulary)?;
    let first: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let second: SubscriptionId = namespace.urn_text("Subscription", 2).parse()?;
    let building = building_registration_subscription(Some(first.clone()), uri, Activation::Unspecified, None)?;
    let cleanup = create_registration_subscription(&client, &building, uri).await?;
    let watching = Subscription::registration_builder()
        .id(second.clone())
        .watched_attributes(["name", "subCategory"])
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .build()?;
    let cleanup = cleanup.absorbing(create_registration_subscription(&client, &watching, uri).await?);
    Ok((vocabulary, [first, second], cleanup))
}

/// `Setup Initial Context Source Registration Subscriptions` of the paging cases: the pair of
/// [`create_registration_subscription_pair`], then the Building subscription `urn:ngsi-ld:Subscription:<ns>-3` with
/// the `geoQ` near the point -8.52, 41.2 within 2000 m of `location`, notifying `uri` (201); answers the vocabulary
/// the three ids and the cleanup that deletes the three subscriptions.
pub(crate) async fn create_registration_subscription_triple(
    tenant: &BrokerClient,
    namespace: &Namespace,
    uri: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 3], Cleanup)> {
    let (vocabulary, [first, second], cleanup) = create_registration_subscription_pair(tenant, namespace, uri).await?;
    let third: SubscriptionId = namespace.urn_text("Subscription", 3).parse()?;
    let near = Subscription::registration_builder()
        .id(third.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .geo_q(geoquery::near_max(2000, point(-8.52, 41.2)?).geoproperty("location").build()?)
        .build()?;
    let cleanup = cleanup.absorbing(create_registration_subscription(&ld_json_body_client(tenant, &vocabulary)?, &near, uri).await?);
    Ok((vocabulary, [first, second, third], cleanup))
}

/// The members of a retrieved Context Source Registration Subscription left uncompared: the notification state the
/// broker reports and the `jsonldContext` it records (clause 5.11.2.4).
pub(crate) const REGISTRATION_SUBSCRIPTION_STATE: [&str; 6] = ["status", "lastFailure", "lastNotification", "timesFailed", "timesSent", "jsonldContext"];

/// `body` without the members `names` at its root: the members a case leaves uncompared, removed so the root's other
/// members are counted exactly.
pub(crate) fn without_members(mut body: Value, names: &[&str]) -> Value {
    if let Some(members) = body.as_object_mut() {
        for name in names {
            members.remove(*name);
        }
    }
    body
}

/// `Retrieve Context Source Registration Subscription` of `id` with the `@context` and `Accept` that `client` carries,
/// answered as the JSON body of the response.
pub(crate) async fn registration_subscription_json(client: &BrokerClient, id: &SubscriptionId) -> Result<Value> {
    let received = client.with_responses().registration_subscriptions().retrieve(id).send().await?;
    Ok(json_body(&received)?)
}

/// The subscription `retrieve` reads, once its `notification` member records a delivery (`lastNotification` present),
/// or as read after the [`delivery_deadline`] when it records none, for the caller's assertions to report.
///
/// Clause 5.8.6 updates the delivery members once the notification exchange completes: the endpoint has the request
/// before the broker has the endpoint's answer, so a retrieval right after the endpoint received the notification can
/// precede the update.
pub(crate) async fn once_delivery_recorded<F, Fut>(retrieve: F) -> Result<Value>
where
    F: Fn() -> Fut + Sync,
    Fut: Future<Output = Result<Value>> + Send,
{
    let recorded = eventually(delivery_deadline(), || async {
        match retrieve().await {
            Ok(body) if body.pointer("/notification/lastNotification").is_some() => Poll::Ready(body),
            Ok(body) => Poll::Pending(body.to_string()),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await;
    match recorded {
        Ok(body) => Ok(body),
        Err(_) => retrieve().await,
    }
}

/// Requires member `name` of `body` to be absent or a date: a notification instant a broker has not set yet is an
/// absent member.
pub(crate) fn expect_absent_or_date(body: &Value, name: &str) {
    match body.get(name) {
        None => {}
        Some(other) => expect_that!(other.as_str().map(is_ngsild_date), some(eq(true)), "{name} is a date"),
    }
}

/// Requires member `name` of `body` to be absent or an integer: a notification count a broker has not set yet is an
/// absent member.
pub(crate) fn expect_absent_or_integer(body: &Value, name: &str) {
    match body.get(name) {
        None => {}
        Some(other) => expect_that!(other.is_i64() || other.is_u64(), eq(true), "{name} is an integer"),
    }
}

/// `Update Context Source Registration Subscription` of `id` with `document` as its body, sent by `client` in the
/// context it carries. The typed operation supplies the target and the headers; the empty fragment it names is
/// replaced by `document`.
pub(crate) fn update_registration_subscription_request(client: &BrokerClient, id: &SubscriptionId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = SubscriptionFragment::registration_builder().build()?;
    Ok(client.unchecked(&UpdateSubscription { id, fragment: &placeholder })?.document(document))
}

/// The body of an Update Context Source Registration Subscription whose only member is a keyValues `notification` to
/// `uri`.
pub(crate) fn registration_notification_fragment(uri: &url::Url) -> Result<UncheckedDocument> {
    let fragment = SubscriptionFragment::registration_builder().build()?;
    Ok(UncheckedDocument::try_from(fragment.render())?
        .remove_member("type")
        .set_member("notification", &key_values_notification(uri)?))
}
