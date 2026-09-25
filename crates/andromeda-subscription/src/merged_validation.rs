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
/// `timeInterval` or `throttling` that is not greater than zero.
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        fields::extract_create_fields,
        merged_validation::validate_merged_subscription,
        subscription_validation_reason::SubscriptionValidationReason,
        variant::DatasetIdPolicy,
    };
    use serde_json::json;

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
