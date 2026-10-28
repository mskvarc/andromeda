//! The removal of the server-side state a test creates, whatever way the test ends.
//!
//! A test that leaves a registration behind changes the answer of every unscoped request of every later test in the
//! tenant: the broker forwards such a request to every registration it holds. A test's own teardown requests stop at
//! the first failed assertion before them, so the state a test creates is also held by a [`Cleanup`], whose `Drop`
//! removes it when the test returns, fails through `?`, or panics.
//!
//! `Drop` cannot await, and the test's runtime cannot serve a request while its thread waits in `Drop`. The removal
//! therefore runs on a thread of its own, with a runtime of its own and a client of its own (a pooled connection of
//! the test's client is driven by the test's runtime), and `Drop` joins that thread. The removal is bounded by
//! [`Cleanup::within`]; its answers are not checked: the test's own teardown requests check theirs, and a removal of
//! something already removed answers 404.

use crate::broker::{BrokerClient, client_at};
use ngsi_ld::{
    model::{ContextId, EntityId, Input, Tenant},
    query::Locality,
    registration::{CSourceRegistration, RegistrationId},
    subscription::{RegistrationSubscription, Subscription, SubscriptionId},
};
use std::{
    mem,
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::{runtime::Builder, time::timeout};
use url::Url;

/// The longest the removals of one [`Cleanup`] may take, by default.
pub const CLEANUP_BUDGET: Duration = Duration::from_secs(30);

/// One piece of server-side state a [`Cleanup`] removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// A context source registration, deleted with Delete Context Source Registration.
    Registration(RegistrationId),
    /// An entity subscription, deleted with Delete Subscription.
    Subscription(SubscriptionId),
    /// A context source registration subscription, deleted with Delete Context Source Registration Subscription.
    RegistrationSubscription(SubscriptionId),
    /// An entity, deleted with `local=true` so that no registration receives the deletion.
    Entity(EntityId),
    /// A stored `@context`, deleted with Delete and Reload @context.
    Context(ContextId),
}

/// The server-side state a test created, removed when the value is dropped.
///
/// Removals run in the reverse order they were added: a registration added after the entities it forwards is deleted
/// before them.
#[derive(Debug)]
#[must_use = "a Cleanup removes its state when dropped: bind it to a named variable that lives until the test ends"]
pub struct Cleanup {
    /// The API root of the broker the state lives on.
    api_root: Url,
    /// The tenant the state lives in; `None` is the default tenant.
    tenant: Option<Tenant>,
    /// The longest the removals may take together.
    budget: Duration,
    /// What to remove, in the order it was added.
    removals: Vec<Removal>,
}

impl Cleanup {
    /// A cleanup of the state `client` creates: on its broker and in its tenant, with nothing to remove yet.
    pub fn of(client: &BrokerClient) -> Cleanup {
        Cleanup {
            api_root: client.endpoint().base_url().clone(),
            tenant: client.tenant().cloned(),
            budget: CLEANUP_BUDGET,
            removals: Vec::new(),
        }
    }

    /// The cleanup with `budget` as the longest its removals may take together.
    pub const fn within(mut self, budget: Duration) -> Cleanup {
        self.budget = budget;
        self
    }

    /// Adds `removal` to what is removed when the cleanup is dropped.
    pub fn remove(&mut self, removal: Removal) {
        self.removals.push(removal);
    }

    /// The cleanup with `removal` added.
    pub fn removing(mut self, removal: Removal) -> Cleanup {
        self.remove(removal);
        self
    }

    /// The cleanup with the registration `id` added. The id is cloned: the cleanup outlives the test's values.
    pub fn registration(self, id: &RegistrationId) -> Cleanup {
        self.removing(Removal::Registration(id.clone()))
    }

    /// The cleanup with the id of `registration` added, when it names one.
    pub fn registration_of(self, registration: &CSourceRegistration<Input>) -> Cleanup {
        match registration.id() {
            Some(id) => self.registration(id),
            None => self,
        }
    }

    /// The cleanup with the entity subscription `id` added. The id is cloned: the cleanup outlives the test's values.
    pub fn subscription(self, id: &SubscriptionId) -> Cleanup {
        self.removing(Removal::Subscription(id.clone()))
    }

    /// The cleanup with the id of the entity subscription `subscription` added, when it names one.
    pub fn subscription_of(self, subscription: &Subscription<Input>) -> Cleanup {
        match subscription.id() {
            Some(id) => self.subscription(id),
            None => self,
        }
    }

    /// The cleanup with the registration subscription `id` added. The id is cloned: the cleanup outlives the test's
    /// values.
    pub fn registration_subscription(self, id: &SubscriptionId) -> Cleanup {
        self.removing(Removal::RegistrationSubscription(id.clone()))
    }

    /// The cleanup with the id of the registration subscription `subscription` added, when it names one.
    pub fn registration_subscription_of(self, subscription: &Subscription<Input, RegistrationSubscription>) -> Cleanup {
        match subscription.id() {
            Some(id) => self.registration_subscription(id),
            None => self,
        }
    }

    /// The cleanup with the entity `id` added. The id is cloned: the cleanup outlives the test's values.
    pub fn entity(self, id: &EntityId) -> Cleanup {
        self.removing(Removal::Entity(id.clone()))
    }

    /// The cleanup with every removal of `other` added after its own, `other` emptied: its tenant and broker must be
    /// this cleanup's.
    pub fn absorbing(mut self, mut other: Cleanup) -> Cleanup {
        self.removals.append(&mut other.removals);
        self
    }

    /// What the cleanup removes, in the order it was added.
    #[must_use]
    pub fn removals(&self) -> &[Removal] {
        &self.removals
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let removals = mem::take(&mut self.removals);
        if removals.is_empty() {
            return;
        }
        let api_root = self.api_root.clone();
        let tenant = self.tenant.take();
        let budget = self.budget;
        let worker = thread::Builder::new()
            .name("cleanup".to_owned())
            .spawn(move || remove_all(api_root, tenant, budget, removals));
        match worker.map(JoinHandle::join) {
            Ok(Ok(()) | Err(_)) | Err(_) => {}
        }
    }
}

/// Removes `removals` in reverse order on the broker at `api_root`, in `tenant`, within `budget`, on a runtime of its
/// own.
fn remove_all(api_root: Url, tenant: Option<Tenant>, budget: Duration, removals: Vec<Removal>) {
    let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
        return;
    };
    runtime.block_on(async move {
        let Ok(client) = client_at(api_root) else {
            return;
        };
        let client = match tenant {
            Some(tenant) => client.with_tenant(tenant),
            None => client,
        };
        match timeout(budget, remove_in_reverse(&client, &removals)).await {
            Ok(()) | Err(_) => {}
        }
    });
}

/// Removes each of `removals` through `client`, the last added first, whatever each answers.
async fn remove_in_reverse(client: &BrokerClient, removals: &[Removal]) {
    for removal in removals.iter().rev() {
        let answered = match removal {
            Removal::Registration(id) => client.registrations().delete(id).send().await.map(drop),
            Removal::Subscription(id) => client.subscriptions().delete(id).send().await.map(drop),
            Removal::RegistrationSubscription(id) => client.registration_subscriptions().delete(id).send().await.map(drop),
            Removal::Entity(id) => client.entities().delete(id).locality(Locality::Local).send().await.map(drop),
            Removal::Context(id) => client.contexts().delete(id).send().await.map(drop),
        };
        match answered {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::{BrokerClient, client_at},
        cleanup::{Cleanup, Removal},
    };
    use googletest::prelude::*;
    use http::{Method, StatusCode};
    use std::time::{Duration, Instant};
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{method, path},
    };

    fn broker_at(server: &MockServer) -> Result<BrokerClient> {
        Ok(client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?)
    }

    fn registration() -> Result<Removal> {
        Ok(Removal::Registration("urn:ngsi-ld:ContextSourceRegistration:cleanup-1".parse()?))
    }

    async fn deleted_paths(server: &MockServer) -> Result<Vec<String>> {
        Ok(server
            .received_requests()
            .await
            .or_fail()?
            .iter()
            .filter(|request| request.method == Method::DELETE)
            .map(|request| request.url.path().to_owned())
            .collect())
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_dropped_cleanup_deletes_its_registration() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(StatusCode::NO_CONTENT))
            .mount(&server)
            .await;
        drop(Cleanup::of(&broker_at(&server)?).removing(registration()?));
        verify_that!(
            deleted_paths(&server).await?,
            elements_are![eq("/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:cleanup-1")]
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn removals_run_in_reverse_order_and_entities_are_deleted_locally() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(StatusCode::NOT_FOUND))
            .mount(&server)
            .await;
        let mut cleanup = Cleanup::of(&broker_at(&server)?);
        cleanup.remove(Removal::Entity("urn:ngsi-ld:Vehicle:cleanup-1".parse()?));
        cleanup.remove(Removal::Subscription("urn:ngsi-ld:Subscription:cleanup-1".parse()?));
        cleanup.remove(registration()?);
        drop(cleanup);
        verify_that!(
            deleted_paths(&server).await?,
            elements_are![
                eq("/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:cleanup-1"),
                eq("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:cleanup-1"),
                eq("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:cleanup-1"),
            ]
        )?;
        let local = server.received_requests().await.or_fail()?;
        let entity_deletion = local.iter().find(|request| request.url.path().starts_with("/ngsi-ld/v1/entities/")).or_fail()?;
        verify_that!(entity_deletion.url.query(), some(eq("local=true")))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cleanup_runs_when_the_test_returns_early_through_an_error() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:cleanup-1"))
            .respond_with(ResponseTemplate::new(StatusCode::NO_CONTENT))
            .mount(&server)
            .await;
        let broker = broker_at(&server)?;
        let failing = || -> Result<()> {
            let _cleanup = Cleanup::of(&broker).removing(registration()?);
            verify_that!(1, eq(2))?;
            Ok(())
        };
        expect_that!(failing(), err(anything()));
        verify_that!(deleted_paths(&server).await?, len(eq(1)))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cleanup_keeps_the_tenant_of_its_client() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(StatusCode::NO_CONTENT))
            .mount(&server)
            .await;
        let tenant = broker_at(&server)?.with_tenant("cleanup-tenant".parse()?);
        drop(Cleanup::of(&tenant).removing(registration()?));
        let requests = server.received_requests().await.or_fail()?;
        let deletion = requests.first().or_fail()?;
        verify_that!(
            deletion.headers.get("NGSILD-Tenant").and_then(|value| value.to_str().ok()),
            some(eq("cleanup-tenant"))
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cleanup_whose_broker_does_not_answer_ends_within_its_budget() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(StatusCode::NO_CONTENT).set_delay(Duration::from_secs(30)))
            .mount(&server)
            .await;
        let started = Instant::now();
        drop(Cleanup::of(&broker_at(&server)?).within(Duration::from_millis(300)).removing(registration()?));
        verify_that!(started.elapsed(), lt(Duration::from_secs(5)))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_cleanup_sends_nothing() -> Result<()> {
        let server = MockServer::start().await;
        drop(Cleanup::of(&broker_at(&server)?));
        verify_that!(server.received_requests().await.or_fail()?, is_empty())
    }
}
