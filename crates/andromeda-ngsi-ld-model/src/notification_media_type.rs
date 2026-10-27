//! The media type a notification body is written as (Table 5.2.15-1 `accept`).
//!
//! Table 5.2.15-1 closes `notification.endpoint.accept` to three values, so a
//! subscription naming any other is refused when it is read rather than
//! notified in a type it never asked for. Absent, the notification is
//! `application/json`.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

/// One of the three values Table 5.2.15-1 allows for `endpoint.accept`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, EnumIter)]
pub enum NotificationMediaType {
    /// `application/json`: the `@context` travels in a `Link` header.
    #[default]
    #[serde(rename = "application/json")]
    #[strum(serialize = "application/json")]
    Json,
    /// `application/ld+json`: the `@context` travels in the body.
    #[serde(rename = "application/ld+json")]
    #[strum(serialize = "application/ld+json")]
    JsonLd,
    /// `application/geo+json`: the entities arrive as one `FeatureCollection`
    /// (Table 5.3.1-1). Clause 6.3.9 leaves it out for Context Source
    /// Registration Subscriptions.
    #[serde(rename = "application/geo+json")]
    #[strum(serialize = "application/geo+json")]
    GeoJson,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_media_type::NotificationMediaType;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn each_allowed_value_round_trips_through_its_spelling() {
        for media_type in NotificationMediaType::iter() {
            let wire = serde_json::to_string(&media_type).expect("serialises");
            pretty_assertions::assert_eq!(serde_json::from_str::<NotificationMediaType>(&wire).expect("reads back"), media_type);
            pretty_assertions::assert_eq!(NotificationMediaType::from_str(&media_type.to_string()).expect("parses"), media_type);
        }
    }

    #[test]
    fn the_spellings_are_the_three_table_5_2_15_1_lists() {
        let spellings: Vec<String> = NotificationMediaType::iter().map(|media_type| media_type.to_string()).collect();
        pretty_assertions::assert_eq!(spellings, ["application/json", "application/ld+json", "application/geo+json"]);
    }

    #[test]
    fn a_media_type_outside_the_three_is_refused() {
        for refused in ["application/xml", "text/plain", "application/json; charset=utf-8", "Application/JSON", "*/*"] {
            assert!(
                serde_json::from_value::<NotificationMediaType>(serde_json::json!(refused)).is_err(),
                "{refused} was read"
            );
        }
    }
}
