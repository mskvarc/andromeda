//! The registrations and subscriptions a sequence's requests create, recorded so the sequence deletes them when it
//! ends: a test that leaves one behind would otherwise answer, or be notified for, the requests of tests that run
//! after it on a kept stack.

use crate::{
    broker,
    cleanup::{Cleanup, Removal},
    sequence::Sequence,
};
use http::{HeaderMap, header::LOCATION};
use percent_encoding::percent_decode_str;

impl Sequence {
    /// Records for deletion the registration or subscription a create answered with `headers` made, when `url` is
    /// one of the broker's registration or subscription collections.
    pub(crate) fn remember(&mut self, url: &str, tenant: Option<String>, headers: &HeaderMap) {
        let Ok(broker) = self.fill("{{broker}}") else { return };
        let Some(collection) = url.strip_prefix(&broker).map(|path| path.split('?').next().unwrap_or_default()) else {
            return;
        };
        let Some(id) = headers
            .get(LOCATION)
            .and_then(|location| location.to_str().ok())
            .and_then(|location| location.rsplit('/').next())
            .map(|id| percent_decode_str(id).decode_utf8_lossy().into_owned())
        else {
            return;
        };
        let removal = match collection {
            "/ngsi-ld/v1/csourceRegistrations" => id.parse().ok().map(Removal::Registration),
            "/ngsi-ld/v1/subscriptions" => id.parse().ok().map(Removal::Subscription),
            "/ngsi-ld/v1/csourceSubscriptions" => id.parse().ok().map(Removal::RegistrationSubscription),
            _ => None,
        };
        let Some(removal) = removal else { return };
        if let Some((_, cleanup)) = self.cleanups.iter_mut().find(|(owner, _)| *owner == tenant) {
            cleanup.remove(removal);
            return;
        }
        let Ok(client) = broker::front() else { return };
        let client = match tenant.as_deref().map(str::parse) {
            Some(Ok(named)) => client.with_tenant(named),
            Some(Err(_)) => return,
            None => client,
        };
        self.cleanups.push((tenant, Cleanup::of(&client).removing(removal)));
    }
}
