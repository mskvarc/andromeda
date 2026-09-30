//! The `@context` of an `application/geo+json` notification (study D6).
//!
//! Table 5.3.1-1 makes a `GeoJSON` notification's `data` one `FeatureCollection`,
//! which "will not contain an @context field" when `receiverInfo` asks for
//! `Prefer: body=json`; clauses 4.5.16.3 and 4.5.17.2 put no `@context` on any
//! Feature of a collection. Otherwise the Notification and its `FeatureCollection`
//! each name the notification context.

use crate::{
    context_links::context_link_targets,
    core_contexts::core,
    notification_context::{create_notified_device, next_notification, receiver, subscribe},
    private_tenant::PrivateTenant,
    user_contexts::UserContexts,
};
use serde_json::{Value, json};

/// Without `Prefer`, the Notification and its `FeatureCollection` name the
/// context and no Feature does; with `Prefer: body=json`, none does and the one
/// link names it.
#[tokio::test]
async fn a_geojson_notification_names_the_context_on_the_envelope_and_the_collection_only() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let expected = json!([contexts.terms, core()]);

    let plain = receiver().await;
    subscribe(
        tenant.client(),
        &plain,
        "application/geo+json",
        json!({ "jsonldContext": contexts.terms, "@context": core() }),
    )
    .await;
    let preferring_json = receiver().await;
    let mut prefer = json!({ "jsonldContext": contexts.terms, "@context": core() });
    prefer["notification"] = json!({
        "endpoint": {
            "uri": preferring_json.endpoint_url().expect("an endpoint"),
            "accept": "application/geo+json",
            "receiverInfo": [{ "key": "Prefer", "value": "body=json" }]
        }
    });
    subscribe(tenant.client(), &preferring_json, "application/geo+json", prefer).await;
    create_notified_device(tenant.client()).await;

    let notification: Value = next_notification(&plain).await.json().expect("a JSON notification");
    assert_eq!(notification["@context"], expected, "{notification}");
    assert_eq!(notification["data"]["type"], "FeatureCollection", "{notification}");
    assert_eq!(notification["data"]["@context"], expected, "{notification}");
    for feature in notification["data"]["features"].as_array().expect("features") {
        assert!(feature.get("@context").is_none(), "no Feature names a @context: {notification}");
    }

    let delivered = next_notification(&preferring_json).await;
    assert_eq!(context_link_targets(&delivered.headers), [contexts.terms.as_str()]);
    let body: Value = delivered.json().expect("a JSON notification");
    assert!(body.get("@context").is_none(), "{body}");
    assert!(body["data"].get("@context").is_none(), "{body}");
    plain.shutdown().await.expect("receiver shutdown");
    preferring_json.shutdown().await.expect("receiver shutdown");
    contexts.shutdown().await;
    tenant.finish().await;
}
