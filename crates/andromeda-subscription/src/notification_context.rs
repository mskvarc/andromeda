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
//! absolute URL is stored (Annex C.8). A request that supplied no `@context`
//! is under the core context (clause 5.5.5), which is one URL, so every created
//! subscription holds a `jsonldContext`. A wrapper that cannot be hosted fails
//! the request: storing nothing would notify under the core context alone,
//! which is not the `@context` applicable for the Subscription.

use crate::{error::SubscriptionError, subscription_context::SubscriptionContext};
use andromeda_jsonld::{compound_context_host::WrapperUse, engine::processor::JsonLdEngine, presented_context::ContextLinkTarget};
use andromeda_ngsi_ld_model::clearable::Clearable;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriBuf;

/// Initialise the `jsonldContext` member of a subscription request from the
/// `@context` applicable to the request, when the request names none itself.
/// A create body has nothing stored to delete, so a member it clears names
/// none either.
///
/// A subscription the broker places under the core context is initialised
/// with the core context URL.
///
/// # Errors
///
/// Returns [`SubscriptionError::JsonLd`] when a document the request named
/// cannot be loaded, and [`SubscriptionError::ContextHosting`] when the
/// applicable context needs a wrapper that cannot be hosted.
pub(crate) async fn initialize_notification_context(
    jsonld_context: &mut Clearable<IriBuf>,
    subscription_context: SubscriptionContext<'_>,
    engine: &JsonLdEngine,
) -> Result<(), SubscriptionError> {
    if jsonld_context.as_set().is_some() {
        return Ok(());
    }
    let stored = match subscription_context {
        SubscriptionContext::Request { context, host } => match engine.presented_context(context).await?.link_target() {
            ContextLinkTarget::Reference(url) => url,
            ContextLinkTarget::Hosted(value) => host.host(&value, WrapperUse::StoredReference).await?,
        },
        SubscriptionContext::CoreOnly => IriBuf::from(NGSI_LD_CORE_CONTEXT_URL),
    };
    *jsonld_context = Clearable::Set(stored);
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::SubscriptionError, notification_context::initialize_notification_context, subscription_context::SubscriptionContext};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        compound_context_host::{CompoundContextHost, CompoundContextHostFuture, ContextHostingError, NoCompoundContextHost, WrapperUse},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
    };
    use andromeda_ngsi_ld_model::clearable::Clearable;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::sync::Mutex;

    const WRAPPER: &str = "https://broker.example/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:wrapper";

    /// A host that records what it hosted and answers one fixed URL.
    #[derive(Debug, Default)]
    struct RecordingHost {
        hosted: Mutex<Vec<JsonLiteral>>,
    }

    impl CompoundContextHost for RecordingHost {
        fn host<'a>(&'a self, context: &'a JsonLiteral, _usage: WrapperUse) -> CompoundContextHostFuture<'a> {
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

    /// `context` as the request handler admits it.
    async fn admitted(context: &ContextSpec) -> AdmittedContext {
        // clone: the admission consumes the spec, and the test keeps its own
        admit_user_context(&engine(), SubmittedContext::from(context.clone()))
            .await
            .expect("the context is admitted")
    }

    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    fn stored_context(jsonld_context: Option<&IriBuf>) -> Option<&str> {
        jsonld_context.map(IriBuf::as_str)
    }

    /// Clause 5.8.1.4: a request naming only a core context is answered, and
    /// notified, under the v1.9 core context, stored as the one URL it is.
    #[tokio::test]
    async fn a_presented_single_url_is_stored_verbatim() {
        let mut payload = Clearable::Absent;
        let context = ContextSpec::from_link(iri_ref("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"));
        let host = RecordingHost::default();

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&context).await,
                host: &host,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        assert!(host.hosted.lock().expect("the recording lock").is_empty());
    }

    /// Annex C.8: an inline context is hosted as a wrapper of exactly the
    /// presented value, whose absolute URL is stored.
    #[tokio::test]
    async fn an_inline_context_is_hosted_and_its_url_stored() {
        let mut payload = Clearable::Absent;
        let context = ContextSpec::from_body(JsonLiteral::admit(&json!({ "speed": "https://example.com/speed" }).to_string()).expect("an admissible literal"));
        let host = RecordingHost::default();

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&context).await,
                host: &host,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some(WRAPPER));
        pretty_assertions::assert_eq!(
            *host.hosted.lock().expect("the recording lock"),
            vec![JsonLiteral::admit(&json!([{ "speed": "https://example.com/speed" }, NGSI_LD_CORE_CONTEXT_URL.as_str()]).to_string()).expect("admissible")]
        );
    }

    /// A wrapper that cannot be hosted fails the request rather than leaving
    /// the notifications under the core context alone.
    #[tokio::test]
    async fn a_wrapper_that_cannot_be_hosted_fails_the_request() {
        let mut payload = Clearable::Absent;
        let context = ContextSpec::from_body(JsonLiteral::admit(&json!({ "speed": "https://example.com/speed" }).to_string()).expect("an admissible literal"));

        let result = initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&context).await,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await;

        assert!(
            matches!(result, Err(SubscriptionError::ContextHosting(ContextHostingError::NoHost))),
            "{result:?}"
        );
        assert!(stored_context(payload.as_set()).is_none());
    }

    /// An explicit `jsonldContext` member takes precedence (clause 5.8.1.4).
    #[tokio::test]
    async fn an_explicit_member_takes_precedence_over_the_applicable_context() {
        let mut payload = Clearable::Set(iri_ref("https://example.com/explicit.jsonld"));
        let context = ContextSpec::from_body(JsonLiteral::admit(&json!({ "speed": "https://example.com/speed" }).to_string()).expect("an admissible literal"));

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&context).await,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await
        .expect("untouched");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some("https://example.com/explicit.jsonld"));
    }

    /// Clauses 5.8.1.4 and 5.5.5: a request that supplied no context is under
    /// the core context, which initialises the member.
    #[tokio::test]
    async fn no_supplied_context_initialises_the_member_with_the_core_context() {
        let mut payload = Clearable::Absent;

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&ContextSpec::none()).await,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }

    /// A request whose body `@context` is the JSON null supplied no context
    /// and is under the core context (clause 5.5.5).
    #[tokio::test]
    async fn a_null_body_context_initialises_the_member_with_the_core_context() {
        let mut payload = Clearable::Absent;
        let context = ContextSpec::from_body(JsonLiteral::admit("null").expect("an admissible literal"));

        initialize_notification_context(
            &mut payload,
            SubscriptionContext::Request {
                context: &admitted(&context).await,
                host: &NoCompoundContextHost,
            },
            &engine(),
        )
        .await
        .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }

    /// A subscription the broker places under the core context has nothing to
    /// host and is initialised with the core context URL.
    #[tokio::test]
    async fn a_broker_placed_subscription_is_initialised_with_the_core_context() {
        let mut payload = Clearable::Absent;

        initialize_notification_context(&mut payload, SubscriptionContext::CoreOnly, &engine())
            .await
            .expect("initialised");

        pretty_assertions::assert_eq!(stored_context(payload.as_set()), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    }
}
