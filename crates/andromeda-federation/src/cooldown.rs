//! Cooldown windows after failed forwarded requests (Table 5.2.34-1, clause
//! 6.3.17).
//!
//! A transport failure (no response, timeout, refused connection, DNS or TLS)
//! is a fact about the endpoint, so it holds back every registration that
//! names the endpoint; an error status is the answer to one registration's
//! request, so it holds back that registration only. A window, once open,
//! ignores further failures until it closes: the requests in flight to one
//! endpoint fail together and count as one failure of the episode.

use crate::cooldown_policy::{CooldownEpisode, CooldownPolicy};
use andromeda_ngsi_ld_lexicon::{dereferenceable_uri::DereferenceableUri, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_retry::schedule::BackoffSchedule;
use dashmap::DashMap;
use std::{
    hash::Hash,
    time::{Duration, Instant},
};

/// Which key a failure holds back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureScope {
    /// Every registration naming the failed endpoint.
    Endpoint,
    /// The failed registration only.
    Registration,
}

/// What kind of HTTP answer a source gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnswerKind {
    /// A 2xx, or a 404, which clause 6.3.17 does not count as abnormal.
    Succeeded,
    /// Any other status.
    ErrorStatus,
}

/// One open or lapsed window of an unbroken failure episode.
#[derive(Debug)]
struct FailureWindow {
    /// When the latest failure that counted opened the window.
    armed_at: Instant,
    /// The episode the window belongs to.
    episode: CooldownEpisode,
}

impl FailureWindow {
    /// Whether the broker's own window is still open at `now`.
    fn is_open(&self, now: Instant) -> bool {
        now.duration_since(self.armed_at) < self.episode.window()
    }

    /// What is left at `now` of the window floored at `floor`, the
    /// registration's own cooldown, which Table 5.2.34-1 makes a minimum.
    fn remaining(&self, floor: Duration, now: Instant) -> Option<Duration> {
        // A zero remainder means the window has just closed, so the registration is contacted again.
        self.episode
            .window()
            .max(floor)
            .checked_sub(now.duration_since(self.armed_at))
            .filter(|remaining| !remaining.is_zero())
    }
}

/// The open cooldown windows, by endpoint and by registration.
#[derive(Debug)]
pub(crate) struct CooldownRegistry {
    /// How long the broker holds a failed source back on its own account.
    policy: CooldownPolicy,
    /// Windows opened by transport failures.
    by_endpoint: DashMap<DereferenceableUri, FailureWindow>,
    /// Windows opened by error statuses.
    by_registration: DashMap<Identifier, FailureWindow>,
}

impl CooldownRegistry {
    /// An empty registry applying `policy`.
    pub(crate) fn new(policy: CooldownPolicy) -> CooldownRegistry {
        CooldownRegistry {
            policy,
            by_endpoint: DashMap::new(),
            by_registration: DashMap::new(),
        }
    }

    /// Record that a forwarded request to `registration` failed now.
    ///
    /// Opens a window for the key `scope` names, or the episode's next window
    /// when the key's last one has lapsed; a failure inside an open window
    /// changes nothing. Under [`CooldownPolicy::Off`] only a registration with
    /// its own cooldown is recorded.
    pub(crate) fn record_failure(&self, registration: &ContextSourceRegistration, scope: FailureScope) {
        match scope {
            FailureScope::Endpoint => self.arm(&self.by_endpoint, &registration.access.endpoint, registration),
            FailureScope::Registration => self.arm(&self.by_registration, &registration.id, registration),
        }
    }

    /// Record that `registration` answered now.
    ///
    /// Any answer proves the endpoint reachable and closes its window; a
    /// success also ends the registration's own episode.
    pub(crate) fn record_answer(&self, registration: &ContextSourceRegistration, answer: AnswerKind) {
        self.by_endpoint.remove(&registration.access.endpoint);
        match answer {
            AnswerKind::Succeeded => {
                self.by_registration.remove(&registration.id);
            }
            AnswerKind::ErrorStatus => {}
        }
    }

    /// How long `registration` stays held back, if it is: the longer of its
    /// endpoint's window and its own, each floored at its `management.cooldown`.
    pub(crate) fn remaining(&self, registration: &ContextSourceRegistration) -> Option<Duration> {
        let floor = own_cooldown(registration).unwrap_or(Duration::ZERO);
        let now = Instant::now();
        let endpoint = self
            .by_endpoint
            .get(&registration.access.endpoint)
            .and_then(|window| window.remaining(floor, now));
        let own = self.by_registration.get(&registration.id).and_then(|window| window.remaining(floor, now));
        endpoint.max(own)
    }

    /// Open or advance the window of `key` for a failure of `registration`.
    fn arm<K: Eq + Hash + Clone>(&self, windows: &DashMap<K, FailureWindow>, key: &K, registration: &ContextSourceRegistration) {
        let now = Instant::now();
        if let Some(mut window) = windows.get_mut(key) {
            if !window.is_open(now) {
                window.episode.advance();
                window.armed_at = now;
            }
            return;
        }
        let episode = match self.policy.first_episode() {
            Some(episode) => episode,
            None if own_cooldown(registration).is_some() => CooldownEpisode::new(BackoffSchedule::constant(Duration::ZERO)),
            None => return,
        };
        // clone: the registry owns its keys
        windows.entry(key.clone()).or_insert(FailureWindow { armed_at: now, episode });
    }
}

/// The registration's own `management.cooldown`, if it sets one.
fn own_cooldown(registration: &ContextSourceRegistration) -> Option<Duration> {
    Some(registration.access.management.as_ref()?.cooldown?.as_duration())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        cooldown::{AnswerKind, CooldownRegistry, FailureScope},
        cooldown_policy::CooldownPolicy,
    };
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_retry::{growth::Growth, schedule::BackoffSchedule};
    use serde_json::json;
    use std::{
        thread,
        time::{Duration, Instant},
    };

    fn registration(id: &str, endpoint: &str, cooldown: Option<u32>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": endpoint,
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

    fn a(cooldown: Option<u32>) -> ContextSourceRegistration {
        registration("urn:ngsi-ld:csr:a", "https://peer.example.org", cooldown)
    }

    fn b() -> ContextSourceRegistration {
        registration("urn:ngsi-ld:csr:b", "https://peer.example.org", None)
    }

    fn exponential(first: Duration, max: Duration) -> CooldownPolicy {
        CooldownPolicy::Windowed(BackoffSchedule::new(first, max, Growth::Doubling))
    }

    fn constant(window: Duration) -> CooldownRegistry {
        CooldownRegistry::new(CooldownPolicy::Windowed(BackoffSchedule::constant(window)))
    }

    fn endpoint_window(registry: &CooldownRegistry, registration: &ContextSourceRegistration) -> Duration {
        registry
            .by_endpoint
            .get(&registration.access.endpoint)
            .map(|window| window.episode.window())
            .expect("an endpoint window")
    }

    fn wait_until_lapsed(registry: &CooldownRegistry, registration: &ContextSourceRegistration) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while registry.remaining(registration).is_some() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(registry.remaining(registration).is_none(), "the window never lapsed");
    }

    #[test]
    fn a_registration_without_a_cooldown_is_never_held_back_when_the_policy_is_off() {
        let registry = CooldownRegistry::new(CooldownPolicy::Off);
        let registration = a(None);
        registry.record_failure(&registration, FailureScope::Endpoint);
        assert!(registry.remaining(&registration).is_none());
    }

    #[test]
    fn a_failure_opens_the_window_and_a_success_closes_it() {
        let registry = CooldownRegistry::new(CooldownPolicy::Off);
        let registration = a(Some(60_000));
        assert!(registry.remaining(&registration).is_none());
        registry.record_failure(&registration, FailureScope::Endpoint);
        assert!(registry.remaining(&registration).is_some());
        registry.record_answer(&registration, AnswerKind::Succeeded);
        assert!(registry.remaining(&registration).is_none());
    }

    #[test]
    fn the_broker_policy_holds_back_a_registration_that_sets_no_cooldown() {
        let registry = constant(Duration::from_secs(30));
        let registration = a(None);
        registry.record_failure(&registration, FailureScope::Endpoint);
        let remaining = registry.remaining(&registration).expect("held back");
        assert!(remaining <= Duration::from_secs(30));
    }

    #[test]
    fn a_transport_failure_holds_back_every_registration_on_the_endpoint() {
        let registry = constant(Duration::from_secs(30));
        let elsewhere = registration("urn:ngsi-ld:csr:c", "https://other.example.org", None);
        registry.record_failure(&a(None), FailureScope::Endpoint);
        assert!(registry.remaining(&b()).is_some());
        assert!(registry.remaining(&elsewhere).is_none());
    }

    #[test]
    fn an_error_status_holds_back_only_its_own_registration() {
        let registry = constant(Duration::from_secs(30));
        let registration = a(None);
        registry.record_answer(&registration, AnswerKind::ErrorStatus);
        registry.record_failure(&registration, FailureScope::Registration);
        assert!(registry.remaining(&registration).is_some());
        assert!(registry.remaining(&b()).is_none());
    }

    #[test]
    fn any_answer_lifts_the_endpoint_window() {
        let registry = constant(Duration::from_secs(30));
        let (first, second) = (a(None), b());
        registry.record_failure(&first, FailureScope::Endpoint);
        registry.record_answer(&second, AnswerKind::ErrorStatus);
        registry.record_failure(&second, FailureScope::Registration);
        assert!(registry.remaining(&first).is_none());
        assert!(registry.remaining(&second).is_some());
    }

    #[test]
    fn the_registration_cooldown_is_a_floor_under_the_broker_window() {
        let registry = constant(Duration::from_secs(1));
        let registration = a(Some(60_000));
        registry.record_failure(&registration, FailureScope::Endpoint);
        assert!(registry.remaining(&registration).expect("held back") > Duration::from_secs(59));
    }

    #[test]
    fn failures_inside_an_active_window_do_not_advance_the_backoff() {
        let registry = CooldownRegistry::new(exponential(Duration::from_secs(1), Duration::from_secs(64)));
        let registration = a(None);
        (0..16).for_each(|_| registry.record_failure(&registration, FailureScope::Endpoint));
        pretty_assertions::assert_eq!(endpoint_window(&registry, &registration), Duration::from_secs(1));
        assert!(registry.remaining(&registration).expect("held back") <= Duration::from_secs(1));
    }

    #[test]
    fn the_next_episode_failure_doubles_the_window() {
        let first = Duration::from_millis(10);
        let registry = CooldownRegistry::new(exponential(first, Duration::from_secs(1)));
        let registration = a(None);
        registry.record_failure(&registration, FailureScope::Endpoint);
        wait_until_lapsed(&registry, &registration);
        registry.record_failure(&registration, FailureScope::Endpoint);
        pretty_assertions::assert_eq!(endpoint_window(&registry, &registration), first * 2);
        assert!(registry.remaining(&registration).expect("held back") <= first * 2);
    }

    #[test]
    fn a_success_resets_the_backoff_to_its_first_window() {
        let first = Duration::from_millis(10);
        let registry = CooldownRegistry::new(exponential(first, Duration::from_secs(1)));
        let registration = a(None);
        registry.record_failure(&registration, FailureScope::Endpoint);
        wait_until_lapsed(&registry, &registration);
        registry.record_failure(&registration, FailureScope::Endpoint);
        wait_until_lapsed(&registry, &registration);
        registry.record_answer(&registration, AnswerKind::Succeeded);
        registry.record_failure(&registration, FailureScope::Endpoint);
        pretty_assertions::assert_eq!(endpoint_window(&registry, &registration), first);
    }
}
