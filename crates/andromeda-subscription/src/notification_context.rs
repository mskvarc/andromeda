//! Initialising a subscription's notification `@context` from the `@context`
//! applicable when it was created (ETSI GS CIM 009 clause 5.8.1.4).
//!
//! Clause 5.8.1.4: "The @context to be used for sending Notifications related
//! to this Subscription shall be the one specified in the jsonldContext field.
//! If not present, the jsonldContext field shall be initialized with the
//! @context applicable for the Subscription (if @context is also not present in
//! the Subscription, see clause 5.5.5)." Table 5.2.12-1 types `jsonldContext`
//! as a single URI, so the applicable `@context` has to be named by one URI.
//!
//! The applicable `@context` is the one a response to the same request names
//! ([`andromeda_jsonld::presented_context`]), and it is reduced to one URI the
//! way a response `Link` is: a presented value that is one URL is stored as it
//! stands, and anything else is hosted as an `ImplicitlyCreated` wrapper whose
//! absolute URL is stored (Annex C.8). A wrapper that cannot be hosted fails
//! the request: storing nothing would notify under the core context alone,
//! which is not the `@context` applicable for the Subscription.

use crate::{error::SubscriptionError, subscription_context::SubscriptionContext};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, presented_context::ContextLinkTarget, request_context::RequestContext};
use serde_json::Value;

/// Initialise the `jsonldContext` member of a subscription payload from the
/// `@context` applicable to the request, when the payload names none itself.
///
/// A request that supplied no context of its own, and a subscription the
/// broker places under the core context, leave the member absent, so its
/// notifications name the core context (clause 5.5.5).
///
/// # Errors
///
/// Returns [`SubscriptionError::JsonLd`] when a document the request named
/// cannot be loaded, and [`SubscriptionError::ContextHosting`] when the
/// applicable context needs a wrapper that cannot be hosted.
pub(crate) async fn initialize_notification_context(
    payload: &mut Value,
    subscription_context: SubscriptionContext<'_>,
    engine: &JsonLdEngine,
) -> Result<(), SubscriptionError> {
    let SubscriptionContext::Request { context, host } = subscription_context else {
        return Ok(());
    };
    if payload.get("jsonldContext").is_some() || names_no_context(context) {
        return Ok(());
    }
    let presented = engine.presented_context(context).await?;
    let stored = match presented.link_target() {
        ContextLinkTarget::Reference(url) => url,
        ContextLinkTarget::Hosted(value) => host.host(&value).await?,
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("jsonldContext".to_owned(), Value::String(stored.into_inner()));
    }
    Ok(())
}

/// Whether the request supplied no `@context` of its own.
const fn names_no_context(context: &ContextSpec) -> bool {
    match context.request_context() {
        RequestContext::Default | RequestContext::Body(Value::Null) => true,
        RequestContext::Link(_) | RequestContext::Body(Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::SubscriptionError, notification_context::initialize_notification_context, subscription_context::SubscriptionContext};
    use andromeda_jsonld::{
        compound_context_host::{CompoundContextHost, CompoundContextHostFuture, ContextHostingError, NoCompoundContextHost},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    const WRAPPER: &str = "https://broker.example/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:wrapper";

    /// A host that records what it hosted and answers one fixed URL.
    #[derive(Debug, Default)]
    struct RecordingHost {
        hosted: Mutex<Vec<Value>>,
    }

    impl CompoundContextHost for RecordingHost {
        fn host<'a>(&'a self, context: &'a Value) -> CompoundContextHostFuture<'a> {
            Box::pin(async move {
                self.hosted.lock().expect("the recording lock").push(context.clone()); // clone: the record keeps its own copy
                Ok(IriBuf::new(WRAPPER.to_owned()).expect("a valid URL"))
            })
        }
    }

    /// An engine that resolves only what is embedded; every other document is
    /// unavailable.
    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds")
    }

    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    fn stored_context(payload: &Value) -> Option<&str> {
        payload.get("jsonldContext").and_then(Value::as_str)
    }

    /// Clause 5.8.1.4: a request naming only a core context is answered, and
    /// notified, under the v1.9 core context, stored as the one URL it is.
    #[tokio::test]
    async fn a_presented_single_url_is_stored_verbatim() {
        let mut payload = json!({});
        let context = ContextSpec::from_link(iri_ref("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"));
        let host = RecordingHost::default();

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &context,
                host: &host,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(&payload), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        assert!(host.hosted.lock().expect("the recording lock").is_empty());
    }

    /// Annex C.8: an inline context is hosted as a wrapper of exactly the
    /// presented value, whose absolute URL is stored.
    #[tokio::test]
    async fn an_inline_context_is_hosted_and_its_url_stored() {
        let mut payload = json!({});
        let context = ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }));
        let host = RecordingHost::default();

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &context,
                host: &host,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(&payload), Some(WRAPPER));
        pretty_assertions::assert_eq!(
            *host.hosted.lock().expect("the recording lock"),
            vec![json!([{ "speed": "https://example.com/speed" }, NGSI_LD_CORE_CONTEXT_URL.as_str()])]
        );
    }

    /// A wrapper that cannot be hosted fails the request rather than leaving
    /// the notifications under the core context alone.
    #[tokio::test]
    async fn a_wrapper_that_cannot_be_hosted_fails_the_request() {
        let mut payload = json!({});
        let context = ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }));

        let result = initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &context,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await;

        assert!(
            matches!(result, Err(SubscriptionError::ContextHosting(ContextHostingError::NoHost))),
            "{result:?}"
        );
        assert!(stored_context(&payload).is_none());
    }

    /// Clause 5.5.6: a named document that cannot be loaded fails the request.
    #[tokio::test]
    async fn a_named_document_that_cannot_be_loaded_fails_the_request() {
        let mut payload = json!({});
        let context = ContextSpec::from_link(iri_ref("https://example.com/unreachable.jsonld"));

        let host = RecordingHost::default();
        let result = initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &context,
                host: &host,
            },
            &engine(),
        )
        .await;

        assert!(matches!(result, Err(SubscriptionError::JsonLd(_))), "{result:?}");
    }

    /// An explicit `jsonldContext` member takes precedence (clause 5.8.1.4).
    #[tokio::test]
    async fn an_explicit_member_takes_precedence_over_the_applicable_context() {
        let mut payload = json!({"jsonldContext": "https://example.com/explicit.jsonld"});
        let context = ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }));

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &context,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await
        .expect("untouched");

        pretty_assertions::assert_eq!(stored_context(&payload), Some("https://example.com/explicit.jsonld"));
    }

    /// Clause 5.5.5: a request that supplied no context leaves the member
    /// absent.
    #[tokio::test]
    async fn no_supplied_context_leaves_the_field_absent() {
        let mut payload = json!({});

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &ContextSpec::none(),
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await
        .expect("untouched");

        assert!(stored_context(&payload).is_none(), "{payload}");
    }

    /// A subscription the broker places under the core context has nothing to
    /// host and leaves the member absent.
    #[tokio::test]
    async fn a_broker_placed_subscription_leaves_the_field_absent() {
        let mut payload = json!({});

        initialize_notification_context(&mut payload, SubscriptionContext::CoreOnly, &engine())
            .await
            .expect("untouched");

        assert!(stored_context(&payload).is_none(), "{payload}");
    }
}
