//! The `@context` of an `application/ld+json` notification body.

use crate::{
    core_contexts::{CORE_V18, CORE_VERSIONLESS, core, core_value},
    notification_context::{create_notified_device, next_notification, receiver, subscribe},
    private_tenant::PrivateTenant,
    user_contexts::UserContexts,
};
use serde_json::{Value, json};

/// Subscribe with the extra members `extra`, create one notified `Device`,
/// and return the `@context` the Notification and its Entity named.
async fn notified_under(tenant: &PrivateTenant, extra: Value) -> (Value, Value) {
    let receiver = receiver().await;
    subscribe(tenant.client(), &receiver, "application/ld+json", extra).await;
    create_notified_device(tenant.client()).await;
    let notification: Value = next_notification(&receiver).await.json().expect("a JSON notification");
    receiver.shutdown().await.expect("receiver shutdown");
    (notification["@context"].clone(), notification["data"][0]["@context"].clone()) // clone: the notification is dropped after this
}

/// Clause 5.5.5: a subscription created under the core
/// `@context` alone, without `jsonldContext`, is notified under the core
/// `@context` alone, on the Notification and on every Entity.
#[tokio::test]
async fn a_subscription_without_a_user_context_is_notified_under_the_core_context() {
    let tenant = PrivateTenant::provisioned().await;

    let (envelope, entity) = notified_under(&tenant, json!({ "@context": core() })).await;

    assert_eq!(envelope, core_value());
    assert_eq!(entity, core_value());
    tenant.finish().await;
}

/// A `jsonldContext` ending with the core
/// `@context` is named alone.
#[tokio::test]
async fn a_jsonld_context_ending_with_the_core_context_is_named_alone() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;

    let (envelope, entity) = notified_under(&tenant, json!({ "jsonldContext": contexts.wrapper, "@context": core() })).await;

    assert_eq!(envelope, json!(contexts.wrapper));
    assert_eq!(entity, json!(contexts.wrapper));
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A `jsonldContext` without the core `@context` is followed by it.
#[tokio::test]
async fn a_jsonld_context_without_the_core_context_is_followed_by_it() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;

    let (envelope, entity) = notified_under(&tenant, json!({ "jsonldContext": contexts.terms, "@context": core() })).await;

    assert_eq!(envelope, json!([contexts.terms, core()]));
    assert_eq!(entity, json!([contexts.terms, core()]));
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A `jsonldContext` that is itself a core release,
/// or the versionless alias, is the subscriber's explicit choice and is named
/// as written (clause 5.8.1.4).
#[tokio::test]
async fn a_core_release_as_jsonld_context_is_named_as_written() {
    let tenant = PrivateTenant::provisioned().await;

    for release in [CORE_V18, CORE_VERSIONLESS, core()] {
        let (envelope, entity) = notified_under(&tenant, json!({ "jsonldContext": release, "@context": core() })).await;
        assert_eq!(envelope, json!(release), "{release}");
        assert_eq!(entity, json!(release), "{release}");
    }
    tenant.finish().await;
}
