//! The registration a context source notification carries (clause 5.11.7).
//!
//! A notification should carry only the `RegistrationInfo` elements the
//! subscription matches rather than the whole registration, so the
//! registration is serialised with its `information` trimmed to them.

use crate::error::NotificationError;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_subscription::{compiled::CompiledCsrSubscription, csource_matching::matching_information};
use serde_json::Value;

/// The registration as the notification carries it: its `information`
/// trimmed to the elements `sub` matches (clause 5.11.7).
pub(crate) fn notified_registration(registration: &ContextSourceRegistration, sub: &CompiledCsrSubscription) -> Result<Value, NotificationError> {
    let mut value = serde_json::to_value(registration).map_err(|error| NotificationError::representation(error.to_string()))?;
    let information = matching_information(sub, registration)
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NotificationError::representation(error.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("information".to_string(), Value::Array(information));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use serde_json::json;
    use url::Url;

    fn subscription(raw_type: &str) -> CompiledCsrSubscription {
        CompiledCsrSubscription {
            id: "urn:ngsi-ld:Subscription:csource".parse().expect("valid URI"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: vec![serde_json::from_value(json!({"type": raw_type})).expect("selector parses")],
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: None,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
            expires_at: None,
            compiled_csf: None,
        }
    }

    /// Clause 5.11.7: the notification carries only the matching
    /// `RegistrationInfo` elements.
    #[test]
    fn the_notified_registration_keeps_only_the_matching_information() {
        let registration: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "information": [
                {"entities": [{"type": "https://example.com/Vehicle"}]},
                {"entities": [{"type": "https://example.com/Building"}]}
            ],
            "endpoint": "https://source.example.com/",
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("registration parses");
        let value = notified_registration(&registration, &subscription("https://example.com/Vehicle")).expect("serialises");
        pretty_assertions::assert_eq!(value["information"], json!([{"entities": [{"type": "https://example.com/Vehicle"}]}]));
        pretty_assertions::assert_eq!(value["id"], json!("urn:ngsi-ld:ContextSourceRegistration:1"));
    }
}
