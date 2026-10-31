//! The subscription operations of the conformance tests: create from a document, the Building subscriptions the tests
//! set up, and update.

use crate::{
    entity_operations::ld_json_body_client,
    subscription_payloads::{key_values_document, key_values_notification},
};
use andromeda_test_suite::{broker::BrokerClient, cleanup::Cleanup, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        UncheckedRequest,
        operation::{CreateSubscription, RegisterContextSource, UpdateSubscription},
    },
    model::{Input, NgsiLdMediaType, RawJson, Representation},
    query::EntitySelector,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionId},
};

/// `Create Subscription` of `document`, the body `subscription` writes with the members the builder cannot spell, sent
/// with `client` and its `@context`: requires 201, and answers the cleanup that deletes the subscription.
pub(crate) async fn create_subscription_document(client: &BrokerClient, subscription: &Subscription<Input>, document: &UncheckedDocument) -> Result<Cleanup> {
    let cleanup = Cleanup::of(client).subscription_of(subscription);
    let request = client.unchecked(&CreateSubscription { subscription })?.document(document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(cleanup)
}

/// The notification of a subscription in the simplified form, delivered to `uri` as `application/json`.
pub(crate) fn simplified_notification(uri: &url::Url) -> Result<NotificationParams<Input>> {
    let endpoint = Endpoint::builder(uri.as_str()).accept(NgsiLdMediaType::Json).build()?;
    Ok(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
}

/// Creates the Building subscription `id` with a `timeInterval` of 5 seconds and a keyValues notification to `uri`, in
/// the context `client` carries, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn create_timed_building_subscription(client: &BrokerClient, id: &SubscriptionId, uri: &url::Url) -> Result<Cleanup> {
    let subscription = Subscription::builder()
        .id(id.clone())
        .time_interval(5)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(uri)?)
        .build()?;
    create_subscription_document(client, &subscription, &key_values_document(&subscription, uri)?).await
}

/// Creates the Building subscription `id` with `isActive` false, no time interval and a keyValues notification to
/// `uri`, in the context `client` carries, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn create_paused_building_subscription(client: &BrokerClient, id: &SubscriptionId, uri: &url::Url) -> Result<Cleanup> {
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(uri)?)
        .build()?;
    let is_active: RawJson = "false".parse()?;
    let document = key_values_document(&subscription, uri)?.set_member("isActive", &is_active);
    create_subscription_document(client, &subscription, &document).await
}

/// The setup of the query tests: registers a Context Source Registration and deletes it again, registers the terms
/// `Building`, `name` and `subCategory`, and creates the subscriptions `<ns>-1` (timed), `<ns>-2` (watching `name` and
/// `subCategory`) and `<ns>-3` (paused) in the tenant, answering the vocabulary and their identifiers in that order,
/// and the cleanup that deletes the three subscriptions.
pub(crate) async fn create_query_subscriptions(
    tenant: &BrokerClient,
    namespace: &Namespace,
    endpoint_url: &url::Url,
) -> Result<(Vocabulary, [SubscriptionId; 3], Cleanup)> {
    let registration_id: RegistrationId = namespace.urn_text("ContextSourceRegistration", "provision").parse()?;
    let registration = CSourceRegistration::builder(endpoint_url.as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities(["ProvisioningSentinel"]).build()?])
        .build()?;
    let _provisioning = Cleanup::of(tenant).registration(&registration_id);
    let registered = send_to(tenant, &RegisterContextSource { registration: &registration }, "csourceRegistrations").await;
    verify_that!(&registered, answered(StatusCode::CREATED))?;
    let deleted = tenant.with_responses().registrations().delete(&registration_id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;

    let vocabulary = Vocabulary::register(tenant, namespace, &["Building", "name", "subCategory"]).await?;
    let client = ld_json_body_client(tenant, &vocabulary)?;
    let [first, second, third] = [1, 2, 3].map(|n| namespace.urn_text("Subscription", n).parse::<SubscriptionId>());
    let (first, second, third) = (first?, second?, third?);
    let cleanup = create_timed_building_subscription(&client, &first, endpoint_url).await?;
    let watched = Subscription::builder()
        .id(second.clone())
        .watched_attributes(["name", "subCategory"])
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(simplified_notification(endpoint_url)?)
        .build()?;
    let cleanup = cleanup
        .absorbing(create_subscription_document(&client, &watched, &key_values_document(&watched, endpoint_url)?).await?)
        .absorbing(create_paused_building_subscription(&client, &third, endpoint_url).await?);
    Ok((vocabulary, [first, second, third], cleanup))
}

/// The body of an `Update Subscription` whose only change is a keyValues `notification` to `uri`: the `type` the fragment writes, then the notification.
pub(crate) fn key_values_fragment(uri: &url::Url) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(SubscriptionFragment::builder().build()?.render())?.set_member("notification", &key_values_notification(uri)?))
}

/// Requires the answer to an `Update Subscription` sent as `request` to be the 204 with an empty body.
pub(crate) async fn subscription_updated(client: &BrokerClient, request: UncheckedRequest) -> Result<()> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))?;
    expect_that!(answer?.body().len(), eq(0));
    Ok(())
}

/// `Update Subscription` of `id` with `document` as its body, sent by `client` in the context it carries. The typed
/// operation supplies the target and the headers; the empty fragment it names is replaced by `document`.
pub(crate) fn update_subscription_request(client: &BrokerClient, id: &SubscriptionId, document: &UncheckedDocument) -> Result<UncheckedRequest> {
    let placeholder = SubscriptionFragment::builder().build()?;
    Ok(client.unchecked(&UpdateSubscription { id, fragment: &placeholder })?.document(document))
}
