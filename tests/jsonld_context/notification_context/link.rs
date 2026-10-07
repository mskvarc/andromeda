//! The one `Link` of an `application/json` notification.
//!
//! Clause 6.3.8: an `application/json` notification "shall include a Link
//! header with a reference to the corresponding JSON-LD @context [...] (to the
//! default JSON-LD @context if none available)", and its body carries no
//! `@context`.

use crate::{
    context_links::context_link_targets,
    core_contexts::{CORE_V18, core},
    notification_context::{create_notified_device, next_notification, receiver, subscribe},
    private_tenant::PrivateTenant,
    user_contexts::UserContexts,
};
use serde_json::{Value, json};

/// A json notification names exactly one context link: the subscription's
/// `jsonldContext`, as written, or the core `@context` when the subscription
/// was created under nothing else.
#[tokio::test]
async fn a_json_notification_links_the_jsonld_context_once() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let cases = [
        (json!({ "@context": core() }), core()),
        (json!({ "jsonldContext": contexts.terms, "@context": core() }), contexts.terms.as_str()),
        (json!({ "jsonldContext": contexts.wrapper, "@context": core() }), contexts.wrapper.as_str()),
        (json!({ "jsonldContext": CORE_V18, "@context": core() }), CORE_V18),
    ];

    for (extra, expected) in cases {
        let receiver = receiver().await;
        subscribe(tenant.client(), &receiver, "application/json", extra).await;
        create_notified_device(tenant.client()).await;
        let notification = next_notification(&receiver).await;
        assert_eq!(context_link_targets(&notification.headers), [expected], "{expected}");
        let body: Value = notification.json().expect("a JSON notification");
        assert!(body.get("@context").is_none(), "a json notification body carries no @context: {body}");
        assert!(body["data"][0].get("@context").is_none(), "{body}");
        receiver.shutdown().await.expect("receiver shutdown");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}
