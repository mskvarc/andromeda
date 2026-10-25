//! The clause 5.2.12 rules an update is checked against once it has been merged
//! with the stored subscription.
//!
//! A fragment is validated on its own members first, but a PATCH names only
//! what it changes: adding `timeInterval` to a subscription that already
//! throttles, or `watchedAttributes` to a periodic one, violates Table 5.2.12-1
//! without either member appearing twice in the fragment. The subscription the
//! update would store is therefore validated again as a whole, so such a
//! request is refused as `BadRequestData` rather than reaching the table's
//! check constraints and surfacing as a persistence failure.

use crate::{error::SubscriptionError, fields::SubscriptionWriteValues, subscription_validation_reason::SubscriptionValidationReason};

/// Refuse a merged subscription that Table 5.2.12-1 does not admit.
///
/// # Errors
///
/// Returns [`SubscriptionError::Validation`] when the merged subscription
/// carries `timeInterval` beside `watchedAttributes` or `throttling`, or a
/// `timeInterval` or `throttling` that is not greater than zero, and, unless
/// it is `localOnly`, when it names neither `entities` nor `watchedAttributes`
/// (a deleted `entities` cannot leave it without a selector).
pub fn validate_merged_subscription(values: &SubscriptionWriteValues) -> Result<(), SubscriptionError> {
    if values.time_interval.is_some() && values.watched_attributes.is_some() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithWatchedAttributes));
    }
    if values.time_interval.is_some() && values.throttling.is_some() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithThrottling));
    }
    if values.throttling.is_some_and(|throttling| throttling <= 0.0) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::ThrottlingNotPositive));
    }
    if values.time_interval.is_some_and(|time_interval| time_interval <= 0.0) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalNotPositive));
    }
    validate_selector(values)
}

/// The presence rule closing clause 5.2.12, which an update that deletes
/// `entities` or `localOnly` can break without naming either member again: "At
/// least one of (a) entities or (b) watchedAttributes shall be present, unless
/// the member localOnly is set to true". The second rule of Table 5.2.12-1,
/// `entities` mandatory beside `timeInterval`, needs no check of its own here:
/// `timeInterval` already excludes `watchedAttributes`, so a periodic
/// subscription without `entities` is one without a selector.
fn validate_selector(values: &SubscriptionWriteValues) -> Result<(), SubscriptionError> {
    if values.local_only {
        return Ok(());
    }
    // Table 5.2.12-1 forbids an empty `entities` array, so an empty one names no
    // entities and satisfies neither presence rule.
    let names_entities = values.entities.as_ref().is_some_and(|entities| !entities.is_empty());
    if !names_entities && values.watched_attributes.is_none() {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::SelectorRequired));
    }
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::SubscriptionError,
        fields::SubscriptionWriteValues,
        merged_validation::validate_merged_subscription,
        subscription_validation_reason::SubscriptionValidationReason,
        variant::DatasetIdPolicy,
    };
    use andromeda_ngsi_ld_model::{request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use serde_json::{Value, json};

    /// The write values a create body of `members` describes, over a
    /// notification that names its endpoint.
    fn extract_create_fields(members: &Value, policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
        let mut wire = json!({"notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}}});
        if let (Some(target), Some(extra)) = (wire.as_object_mut(), members.as_object()) {
            target.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        let request = SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable request");
        SubscriptionWriteValues::from_create(request, policy)
    }

    /// Clause 5.2.12, Table 5.2.12-1: a throttled subscription a PATCH gives a
    /// `timeInterval` is refused once merged, though the fragment alone is valid.
    #[test]
    fn a_merged_subscription_with_throttling_and_a_time_interval_is_refused() -> Result<(), SubscriptionError> {
        let mut merged = extract_create_fields(&json!({"throttling": 5}), DatasetIdPolicy::Retain)?;
        merged.time_interval = Some(10.0);
        assert!(matches!(
            validate_merged_subscription(&merged),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithThrottling))
        ));
        Ok(())
    }

    /// Clause 5.2.12, Table 5.2.12-1: `timeInterval` excludes `watchedAttributes`.
    #[test]
    fn a_merged_subscription_with_watched_attributes_and_a_time_interval_is_refused() -> Result<(), SubscriptionError> {
        let mut merged = extract_create_fields(&json!({"watchedAttributes": ["https://example.org/speed"]}), DatasetIdPolicy::Retain)?;
        merged.time_interval = Some(10.0);
        assert!(matches!(
            validate_merged_subscription(&merged),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalWithWatchedAttributes))
        ));
        Ok(())
    }

    #[test]
    fn a_merged_subscription_that_only_throttles_is_admitted() -> Result<(), SubscriptionError> {
        let merged = extract_create_fields(
            &json!({"throttling": 5, "watchedAttributes": ["https://example.org/speed"]}),
            DatasetIdPolicy::Retain,
        )?;
        validate_merged_subscription(&merged)
    }

    /// Clause 5.2.12: a subscription whose `entities` an update deleted, and
    /// that names no `watchedAttributes`, has no selector and is refused once
    /// merged, though the fragment alone names nothing wrong.
    #[test]
    fn a_merged_subscription_without_entities_or_watched_attributes_is_refused() -> Result<(), SubscriptionError> {
        let merged = extract_create_fields(&json!({}), DatasetIdPolicy::Retain)?;
        assert!(matches!(
            validate_merged_subscription(&merged),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::SelectorRequired))
        ));
        Ok(())
    }

    /// Clause 5.2.12: `entities` is mandatory when `timeInterval` is present, so
    /// an update that deleted it from a periodic subscription leaves no
    /// selector and is refused.
    #[test]
    fn a_merged_periodic_subscription_without_entities_is_refused() -> Result<(), SubscriptionError> {
        let merged = extract_create_fields(&json!({"timeInterval": 10}), DatasetIdPolicy::Retain)?;
        assert!(matches!(
            validate_merged_subscription(&merged),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::SelectorRequired))
        ));
        Ok(())
    }

    /// Clause 5.2.12: either presence rule is lifted by `localOnly`, and either
    /// `entities` or `watchedAttributes` satisfies the first.
    #[test]
    fn a_merged_local_subscription_or_one_with_a_selector_is_admitted() -> Result<(), SubscriptionError> {
        validate_merged_subscription(&extract_create_fields(&json!({"localOnly": true}), DatasetIdPolicy::Retain)?)?;
        validate_merged_subscription(&extract_create_fields(&json!({"entities": [{"type": "Vehicle"}]}), DatasetIdPolicy::Retain)?)?;
        validate_merged_subscription(&extract_create_fields(
            &json!({"timeInterval": 10, "entities": [{"type": "Vehicle"}]}),
            DatasetIdPolicy::Retain,
        )?)
    }

    #[test]
    fn a_merged_non_positive_time_interval_is_refused() -> Result<(), SubscriptionError> {
        let mut merged = extract_create_fields(&json!({}), DatasetIdPolicy::Retain)?;
        merged.time_interval = Some(0.0);
        assert!(matches!(
            validate_merged_subscription(&merged),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::TimeIntervalNotPositive))
        ));
        Ok(())
    }
}
