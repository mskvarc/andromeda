//! The subscriptions and notification members the subscription tests send, written as those tests write them.

use googletest::Result;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{DateTime, Input, RawJson, Representation},
    query::EntitySelector,
    subscription::{
        Activation,
        Endpoint,
        NotificationParams,
        RegistrationSubscription,
        Subscription,
        SubscriptionId,
        SubscriptionKind,
        SystemAttributeReporting,
    },
};
use url::Url;

/// The subscription `id` to every Building, notifying `endpoint` in `format`, with no member beyond the entity
/// selector and the notification.
pub(crate) fn building_subscription(id: SubscriptionId, endpoint: Endpoint, format: Representation) -> Result<Subscription<Input>> {
    Ok(Subscription::builder()
        .id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).format(format).build()?)
        .build()?)
}

/// The `notification` member in the `keyValues` spelling of the format, with an endpoint at `uri` that accepts
/// `application/json`.
pub(crate) fn key_values_notification(uri: &Url) -> Result<RawJson> {
    key_values_notification_reporting(uri, SystemAttributeReporting::Unspecified)
}

/// The keyValues notification to `uri` with the `sysAttrs` member `reporting` writes, and none for
/// [`SystemAttributeReporting::Unspecified`].
pub(crate) fn key_values_notification_reporting(uri: &Url, reporting: SystemAttributeReporting) -> Result<RawJson> {
    let sys_attrs = match reporting {
        SystemAttributeReporting::Unspecified => "",
        SystemAttributeReporting::Include => r#","sysAttrs":true"#,
        SystemAttributeReporting::Omit => r#","sysAttrs":false"#,
    };
    Ok(format!(r#"{{"format":"keyValues","endpoint":{{"uri":"{uri}","accept":"application/json"}}{sys_attrs}}}"#).parse()?)
}

/// The document of `subscription` with its notification written as [`key_values_notification_reporting`] writes it.
pub(crate) fn key_values_document_reporting(subscription: &Subscription<Input>, uri: &Url, reporting: SystemAttributeReporting) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(subscription.render())?.set_member("notification", &key_values_notification_reporting(uri, reporting)?))
}

/// The document of `subscription` with its notification as [`key_values_notification`] writes it.
pub(crate) fn key_values_document<K: SubscriptionKind>(subscription: &Subscription<Input, K>, uri: &Url) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(subscription.render())?.set_member("notification", &key_values_notification(uri)?))
}

/// The Context Source Registration Subscription to every Building, named `id` when it is given, notifying `uri` (the
/// notification [`key_values_document`] writes in its place), in the state `activation` writes and expiring at
/// `expires_at` when it names an instant.
pub(crate) fn building_registration_subscription(
    id: Option<SubscriptionId>,
    uri: &Url,
    activation: Activation,
    expires_at: Option<DateTime>,
) -> Result<Subscription<Input, RegistrationSubscription>> {
    Ok(Subscription::registration_builder()
        .maybe_id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .activation(activation)
        .maybe_expires_at(expires_at)
        .build()?)
}
