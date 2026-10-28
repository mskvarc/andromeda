//! `Create initial ImplicitlyCreated @context`: a subscription whose `@context` lists two remote documents, which the
//! broker downloads as `Cached` contexts and merges into an `ImplicitlyCreated` one (clauses 5.13.1 and 5.13.2.4).
//!
//! The remote documents are served by the case's own server, so the cache entries they make are the case's alone.
//! The setup runs in two steps, [`ImplicitSubscription::create`] and [`ImplicitSubscription::retrieve`], because some
//! cases send a request between them.

use andromeda_test_suite::{
    broker::BrokerClient,
    cleanup::Cleanup,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
    teardown::AnyAnswer,
    wire,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ContextId},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::Value;
use url::Url;

/// The path the suite's test `@context` is served at.
const TEST_CONTEXT: &str = "fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld";

/// The path the suite's smart data models `@context` is served at.
const SMART_DATA_MODELS: &str = "smartdatamodels.org/context.jsonld";

/// The two remote documents the broker downloads, and the subscription whose `@context` names them.
#[derive(Debug)]
pub(crate) struct ImplicitSubscription {
    /// The server of the two remote documents; it serves as long as the value lives.
    remote: RemoteContexts,
    /// The URL of the remote test `@context`, the identifier of its `Cached` copy.
    test_context: Url,
    /// The URL of the remote smart data models `@context`, the identifier of its `Cached` copy.
    smart_data_models: Url,
    /// The subscription whose `@context` the broker stored.
    subscription: SubscriptionId,
    /// Deletes the subscription however the case ends.
    _cleanup: Cleanup,
}

impl ImplicitSubscription {
    /// Serves the two remote documents, each mapping `T1` into the case's namespace, and creates the subscription to
    /// entities of type `T1` with both in its `@context`, whatever the broker answers (`HTTP *`).
    pub(crate) async fn create(client: &BrokerClient, namespace: &Namespace) -> Result<ImplicitSubscription> {
        let remote = RemoteContexts::start().await?;
        let document = format!(r#"{{"@context": {{"T1": "{}"}}}}"#, namespace.vocabulary_iri("T1")?);
        let test_context = remote.serve(TEST_CONTEXT, &document, Some("application/ld+json")).await?;
        let smart_data_models = remote.serve(SMART_DATA_MODELS, &document, Some("application/ld+json")).await?;
        let subscription: SubscriptionId = namespace.urn_text("subscriptions", "01").parse()?;
        let notify = remote.url("notify/1")?;
        let created = Subscription::builder()
            // The builder owns the identifier; the setup keeps it for the retrieval and the teardown.
            .id(subscription.clone())
            .entities([EntitySelector::new("T1".parse()?)])
            .notification(NotificationParams::builder(Endpoint::builder(notify.as_str()).build()?).build()?)
            .build()?;
        let cleanup = Cleanup::of(client).subscription_of(&created);
        let context = Context::Multiple(vec![
            ContextEntry::Reference(IriRefBuf::new(test_context.to_string())?),
            ContextEntry::Reference(IriRefBuf::new(smart_data_models.to_string())?),
        ]);
        client
            .with_context(context)?
            .with_context_delivery(ContextDelivery::LdJsonBody)
            .subscriptions()
            .create(&created)
            .send()
            .await
            .any_answer();
        Ok(ImplicitSubscription {
            remote,
            test_context,
            smart_data_models,
            subscription,
            _cleanup: cleanup,
        })
    }

    /// Retrieves the subscription with `Accept: */*` and reads the `jsonldContext` the broker reports for it, required
    /// to be present. Answers the setup and the retrieved body.
    pub(crate) async fn retrieve(self, client: &BrokerClient) -> Result<(ImplicitContext, Value)> {
        let retrieved = client
            .with_accept(Accept::any())
            .with_responses()
            .subscriptions()
            .retrieve(&self.subscription)
            .send()
            .await?;
        let body = wire::json_body(&retrieved)?;
        // The subscription lends its `jsonldContext` only; the setup outlives it.
        let url = retrieved.outcome.jsonld_context().or_fail()?.clone();
        let id = url.path_segments().and_then(|mut segments| segments.next_back()).or_fail()?.parse()?;
        Ok((ImplicitContext { setup: self, url, id }, body))
    }

    /// `Delete Initial @context Data`: deletes the subscription, the two `Cached` contexts and `implicit` when given,
    /// whatever the broker answers, then stops serving the remote documents.
    pub(crate) async fn delete(self, client: &BrokerClient, implicit: Option<&ContextId>) -> Result<()> {
        client.subscriptions().delete(&self.subscription).send().await.any_answer();
        client.contexts().delete(&self.test_context.as_str().parse()?).send().await.any_answer();
        client.contexts().delete(&self.smart_data_models.as_str().parse()?).send().await.any_answer();
        if let Some(implicit) = implicit {
            client.contexts().delete(implicit).send().await.any_answer();
        }
        self.remote.stop();
        Ok(())
    }
}

/// The contexts the setup made the broker store.
#[derive(Debug)]
pub(crate) struct ImplicitContext {
    /// The remote documents and the subscription.
    setup: ImplicitSubscription,
    /// The `jsonldContext` the broker reports for the subscription.
    pub(crate) url: Url,
    /// The identifier of the `ImplicitlyCreated` context: the last segment of [`ImplicitContext::url`].
    pub(crate) id: ContextId,
}

impl ImplicitContext {
    /// Both steps of the setup, with nothing between them.
    pub(crate) async fn set_up(client: &BrokerClient, namespace: &Namespace) -> Result<ImplicitContext> {
        Ok(ImplicitSubscription::create(client, namespace).await?.retrieve(client).await?.0)
    }

    /// The URL of the remote test `@context`, the identifier of its `Cached` copy.
    pub(crate) const fn test_context(&self) -> &Url {
        &self.setup.test_context
    }

    /// The URL of the remote smart data models `@context`, the identifier of its `Cached` copy.
    pub(crate) const fn smart_data_models(&self) -> &Url {
        &self.setup.smart_data_models
    }

    /// Serves the `ImplicitlyCreated` context with `Accept: */*`: requires 200 and a JSON object with no
    /// `numberOfHits`, a served document being the stored `@context` alone (clauses 5.13.1 and 5.13.4.4).
    pub(crate) async fn expect_served_without_hits(&self, client: &BrokerClient) -> Result<()> {
        let served = client.with_accept(Accept::any()).with_responses().contexts().serve(&self.id).send().await;
        expect_that!(&served, answered(StatusCode::OK));
        let body = wire::json_body(&served?)?;
        expect_that!(body.as_object(), some(anything()));
        expect_that!(body.get("numberOfHits"), none());
        Ok(())
    }

    /// `Delete Initial @context Data`: deletes the subscription, the two `Cached` contexts and, when `implicit` says
    /// so,
    /// the `ImplicitlyCreated` one, whatever the broker answers, then stops serving the remote documents.
    pub(crate) async fn delete(self, client: &BrokerClient, implicit: ImplicitTeardown) -> Result<()> {
        let implicit = match implicit {
            ImplicitTeardown::Delete => Some(&self.id),
            ImplicitTeardown::Keep => None,
        };
        self.setup.delete(client, implicit).await
    }
}

/// Whether the teardown of [`ImplicitContext`] deletes the `ImplicitlyCreated` context too, as the case's teardown
/// lists it or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImplicitTeardown {
    /// The `ImplicitlyCreated` context is deleted after the `Cached` ones.
    Delete,
    /// The `ImplicitlyCreated` context is left to the broker.
    Keep,
}
