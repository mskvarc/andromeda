//! Per-registration cooldown after a failed forwarded request (clause 5.2.34).

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use dashmap::DashMap;
use std::time::{Duration, Instant};

/// Remembers when each registration last failed, so a registration whose
/// `management.cooldown` has not elapsed is answered with a timeout instead of
/// being contacted again.
#[derive(Debug, Default)]
pub(crate) struct CooldownRegistry {
    /// The instant of the last failure, per registration.
    last_failure: DashMap<Uri, Instant>,
}

impl CooldownRegistry {
    /// Record that a forwarded request to `registration` failed now.
    pub(crate) fn record_failure(&self, registration: &Uri) {
        self.last_failure.insert(registration.clone(), Instant::now()); // clone: the registry owns its keys
    }

    /// Forget a registration's failure once it answers again.
    pub(crate) fn record_success(&self, registration: &Uri) {
        self.last_failure.remove(registration);
    }

    /// How long `registration` stays inside its cooldown window, if it is in one.
    pub(crate) fn remaining(&self, registration: &ContextSourceRegistration) -> Option<Duration> {
        let cooldown = registration.access.management.as_ref()?.cooldown?.as_duration();
        let last_failure = *self.last_failure.get(&registration.id)?;
        let elapsed = last_failure.elapsed();
        (elapsed < cooldown).then(|| cooldown - elapsed)
    }
}

#[cfg(test)]
mod tests {
    use crate::cooldown::CooldownRegistry;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::json;

    fn registration(cooldown: Option<u32>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(cooldown) = cooldown {
            document["management"] = json!({"cooldown": cooldown});
        }
        serde_json::from_value(document).expect("valid registration")
    }

    #[test]
    fn a_registration_without_a_cooldown_is_never_held_back() {
        let registry = CooldownRegistry::default();
        let registration = registration(None);
        registry.record_failure(&registration.id);
        assert!(registry.remaining(&registration).is_none());
    }

    #[test]
    fn a_failure_opens_the_window_and_a_success_closes_it() {
        let registry = CooldownRegistry::default();
        let registration = registration(Some(60_000));
        assert!(registry.remaining(&registration).is_none());
        registry.record_failure(&registration.id);
        assert!(registry.remaining(&registration).is_some());
        registry.record_success(&registration.id);
        assert!(registry.remaining(&registration).is_none());
    }
}
