//! Where a notification carries its JSON-LD `@context` (clause 6.3.8).

use andromeda_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType, prefer::Prefer};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use headers::{Header, HeaderValue};

/// The `receiverInfo` key clause 6.3.8 reads the body preference from.
///
/// RFC 7230 section 3.2 makes header field names case-insensitive, and
/// `receiverInfo` entries become header fields, so the key is matched that way.
const PREFER_KEY: &str = "Prefer";

/// Where the `@context` of one notification belongs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationContextPlacement {
    /// An `@context` member inside the payload body.
    Body,
    /// A `Link` header referencing the context, per the JSON-LD specification
    /// section 6.2.
    LinkHeader,
}

/// Decide where a notification's `@context` goes (clause 6.3.8).
///
/// Clause 6.3.5 makes an `application/ld+json` notification a JSON-LD document
/// that carries its own `@context`, and clause 6.3.8 puts the `application/json`
/// context in a `Link` header. `application/geo+json` is the media type whose
/// answer depends on the subscription: the context goes in the body when
/// `receiverInfo` carries `Prefer: body=ld+json`, when the `Prefer` key is
/// omitted, and when `receiverInfo` does not exist at all -- only an explicit
/// `Prefer: body=json` moves it to the `Link` header. The body is therefore the
/// default for `GeoJSON` notifications, which is the opposite of the clause 6.3.7
/// rule for `GeoJSON` *responses*.
#[must_use]
pub fn notification_context_placement(content_type: ResponseMediaType, receiver_info: Option<&[KeyValuePair]>) -> NotificationContextPlacement {
    match content_type {
        ResponseMediaType::JsonLd => NotificationContextPlacement::Body,
        ResponseMediaType::Json => NotificationContextPlacement::LinkHeader,
        ResponseMediaType::GeoJson => match geo_json_body_preference(receiver_info) {
            GeoJsonBodyPreference::LdJson => NotificationContextPlacement::Body,
            GeoJsonBodyPreference::Json => NotificationContextPlacement::LinkHeader,
        },
    }
}

/// Read the `Prefer` body preference out of a subscription's `receiverInfo`.
///
/// An absent key, and a value RFC 7240 cannot be read as a preference, both
/// leave the default in place, which is what clause 6.3.8 asks for when the
/// `Prefer` key is omitted.
fn geo_json_body_preference(receiver_info: Option<&[KeyValuePair]>) -> GeoJsonBodyPreference {
    receiver_info
        .unwrap_or_default()
        .iter()
        .find(|pair| pair.key.eq_ignore_ascii_case(PREFER_KEY))
        .and_then(|pair| HeaderValue::from_str(&pair.value).ok())
        .and_then(|value| Prefer::decode(&mut [value].iter()).ok())
        .map_or_else(GeoJsonBodyPreference::default, |prefer| prefer.geo_json_body())
}

#[cfg(test)]
mod tests {
    use crate::notification_context_placement::{NotificationContextPlacement, notification_context_placement};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;

    fn receiver_info(key: &str, value: &str) -> Vec<KeyValuePair> {
        vec![KeyValuePair {
            key: key.into(),
            value: value.into(),
        }]
    }

    /// Clause 6.3.5: an `application/ld+json` notification is a JSON-LD
    /// document and carries its `@context` at the top level of the body.
    #[test]
    fn an_ld_json_notification_carries_the_context_in_its_body() {
        assert_eq!(
            notification_context_placement(ResponseMediaType::JsonLd, None),
            NotificationContextPlacement::Body
        );
    }

    /// Clause 6.3.8: "If the target MIME type is `application/json` then the
    /// HTTP notification request shall include a Link header".
    #[test]
    fn a_json_notification_carries_the_context_in_a_link_header() {
        assert_eq!(
            notification_context_placement(ResponseMediaType::Json, None),
            NotificationContextPlacement::LinkHeader
        );
    }

    /// Clause 6.3.8: with `geo+json` and no `receiverInfo` at all, "the HTTP
    /// notification request includes an `@context` element in the payload body".
    #[test]
    fn a_geo_json_notification_without_receiver_info_carries_the_context_in_its_body() {
        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, None),
            NotificationContextPlacement::Body
        );
    }

    /// Clause 6.3.8: the same holds when `receiverInfo` exists but omits `Prefer`.
    #[test]
    fn a_geo_json_notification_without_a_prefer_key_carries_the_context_in_its_body() {
        let info = receiver_info("Authorization", "Bearer token");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::Body
        );
    }

    /// Clause 6.3.8: `Prefer: body=ld+json` is the explicit spelling of the default.
    #[test]
    fn a_geo_json_notification_preferring_ld_json_carries_the_context_in_its_body() {
        let info = receiver_info("Prefer", "body=ld+json");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::Body
        );
    }

    /// Clause 6.3.8: only `Prefer: body=json` moves a `GeoJSON` notification's
    /// context into the `Link` header.
    #[test]
    fn a_geo_json_notification_preferring_json_carries_the_context_in_a_link_header() {
        let info = receiver_info("Prefer", "body=json");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::LinkHeader
        );
    }

    /// RFC 7230 section 3.2 makes header field names case-insensitive, so the
    /// preference is honoured however the subscription spelled the key.
    #[test]
    fn the_prefer_key_is_matched_case_insensitively() {
        let info = receiver_info("prefer", "body=json");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::LinkHeader
        );
    }

    /// A `Prefer` value carrying other RFC 7240 tokens still yields the body
    /// preference it names.
    #[test]
    fn a_prefer_value_with_further_tokens_still_yields_its_body_preference() {
        let info = receiver_info("Prefer", "respond-async, body=json");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::LinkHeader
        );
    }

    /// An unreadable preference leaves the clause 6.3.8 default in place rather
    /// than failing the delivery.
    #[test]
    fn an_unreadable_prefer_value_falls_back_to_the_body() {
        let info = receiver_info("Prefer", "body=xml");

        assert_eq!(
            notification_context_placement(ResponseMediaType::GeoJson, Some(&info)),
            NotificationContextPlacement::Body
        );
    }
}
