//! Forwarded-request oracles shared by the provision forwarding tests.
//!
//! A provision forward reaches the mock Context Source as a normalized NGSI-LD
//! request whose attribute terms may be compact (`speed`) or expanded to a
//! test-suite IRI (`…/speed`). These helpers therefore compare terms by their
//! last path segment, expose a forwarded request's origin-rooted path for
//! fan-out routing, and unwrap the entity list a batch operation forwards
//! (either a bare array or an `{ "entities": [...] }` envelope).

use andromeda_test_mocks::callback_server::CapturedRequest;
use serde_json::Value;
use std::time::Duration;

/// Budget for observing a forwarded provision request. Generous enough for a
/// cold broker to resolve the registration and issue the outbound call.
pub const FORWARD_WAIT: Duration = Duration::from_secs(5);

/// Whether a forwarded body (or a forwarded entity object) carries the attribute
/// named `term`, regardless of whether the broker compacted the term or left it
/// as an expanded IRI. `id`, `type` and `@context` are ordinary keys and match
/// too when passed as `term`.
pub fn attr_present(body: &Value, term: &str) -> bool {
    body.as_object()
        .is_some_and(|map| map.keys().any(|key| key == term || key.rsplit(['/', '#']).next() == Some(term)))
}

/// The origin-rooted path a forwarded request targeted, e.g.
/// `/ngsi-ld/v1/entities`, used to route a fan-out of forwarded requests back to
/// the registration that produced each one.
pub fn request_path(request: &CapturedRequest) -> &str {
    request.uri.path()
}

/// The entities carried by a forwarded batch body, accepting either the bare
/// JSON array the batch endpoints use or an `{ "entities": [...] }` envelope.
pub fn forwarded_entities(body: &Value) -> Vec<&Value> {
    if let Some(array) = body.as_array() {
        return array.iter().collect();
    }
    body.get("entities")
        .and_then(Value::as_array)
        .map(|array| array.iter().collect())
        .unwrap_or_default()
}

/// The forwarded entity whose `id` equals `id`, if present in the list.
pub fn entity_with_id<'a>(entities: &'a [&'a Value], id: &str) -> Option<&'a Value> {
    entities.iter().copied().find(|entity| entity.get("id").and_then(Value::as_str) == Some(id))
}
