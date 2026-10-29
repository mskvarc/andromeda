//! A notified-registration record held in memory, standing in for the tenant
//! database in tests.

use crate::{
    error::NotificationError,
    notified_registration_store::{NotifiedRegistrationStore, Release},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use futures::future::BoxFuture;
use std::{collections::BTreeSet, sync::Mutex};

/// The notified pairs one tenant database holds, shared by every planner built
/// over it as brokers share the database.
#[derive(Default)]
pub(crate) struct InMemoryLedger {
    /// `(subscription, registration)` pairs.
    pairs: Mutex<BTreeSet<(String, String)>>,
}

impl InMemoryLedger {
    /// The `(subscription, registration)` pairs recorded so far.
    pub(crate) fn pairs(&self) -> BTreeSet<(String, String)> {
        self.pairs.lock().expect("ledger lock").clone()
    }
}

impl NotifiedRegistrationStore for InMemoryLedger {
    fn claim<'a>(&'a self, subscription: &'a Identifier, registrations: &'a [Identifier]) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move {
            let mut pairs = self.pairs.lock().expect("ledger lock");
            Ok(registrations
                .iter()
                .filter(|registration| pairs.insert((subscription.to_string(), registration.to_string())))
                .cloned()
                .collect())
        })
    }

    fn release<'a>(&'a self, subscription: &'a Identifier, registration: &'a Identifier) -> BoxFuture<'a, Result<Release, NotificationError>> {
        Box::pin(async move {
            let removed = self
                .pairs
                .lock()
                .expect("ledger lock")
                .remove(&(subscription.to_string(), registration.to_string()));
            Ok(if removed { Release::Released } else { Release::Absent })
        })
    }

    fn notified_subscriptions<'a>(&'a self, registration: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move {
            Ok(self
                .pairs
                .lock()
                .expect("ledger lock")
                .iter()
                .filter(|(_, notified)| notified == registration.as_str())
                .map(|(subscription, _)| identifier(subscription))
                .collect())
        })
    }

    fn registrations_notified_to<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>> {
        Box::pin(async move {
            Ok(self
                .pairs
                .lock()
                .expect("ledger lock")
                .iter()
                .filter(|(owner, _)| owner == subscription.as_str())
                .map(|(_, registration)| identifier(registration))
                .collect())
        })
    }
}

fn identifier(raw: &str) -> Identifier {
    raw.parse().expect("valid URI")
}
