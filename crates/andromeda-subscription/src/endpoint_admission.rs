//! Admitting a subscription's notification endpoint (ETSI GS CIM 009 clause
//! 5.2.15).
//!
//! Which endpoints a broker can reach is a property of the delivery bindings
//! it was built with, not of the subscription resource, so the rule is a trait
//! the composition implements and the lifecycle merely calls. Create and
//! update ask it the same question -- an update fragment that touches only the
//! endpoint has to be admitted exactly as a create carrying the same endpoint
//! would be.

use crate::{endpoint_refusal::EndpointRefusal, error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

/// Reject a subscription whose endpoint names a transport this broker cannot
/// deliver over, or a `notifierInfo` it cannot read.
///
/// Runs against the raw payload rather than a parsed `Subscription` because
/// an update carries a fragment, and the same check has to hold for a create
/// and for a patch that touches only the endpoint.
///
/// # Errors
///
/// Returns [`SubscriptionError::Validation`] when the endpoint `uri` is not a
/// URL, when `notifierInfo` is not an array of key-value pairs, or when the
/// binding refuses the endpoint.
pub(crate) fn validate_endpoint(payload: &Value, validator: &dyn SubscriptionEndpointValidator) -> Result<(), SubscriptionError> {
    let Some(endpoint) = payload.get("notification").and_then(|notification| notification.get("endpoint")) else {
        return Ok(());
    };
    let Some(endpoint_uri) = endpoint.get("uri").and_then(Value::as_str) else {
        return Ok(());
    };
    let endpoint_uri = Url::parse(endpoint_uri).map_err(|source| SubscriptionError::Validation(SubscriptionValidationReason::EndpointUri { source }))?;

    let notifier_info = match endpoint.get("notifierInfo") {
        Some(value) => Some(
            Vec::<KeyValuePair>::deserialize(value).map_err(|source| SubscriptionError::Validation(SubscriptionValidationReason::NotifierInfo { source }))?,
        ),
        None => None,
    };

    validator
        .validate(&endpoint_uri, notifier_info.as_deref())
        .map_err(|refusal| SubscriptionError::Validation(SubscriptionValidationReason::EndpointRejected(refusal)))
}

/// Validates whether endpoint configuration can be served by the selected delivery bindings.
pub trait SubscriptionEndpointValidator: Send + Sync + 'static {
    /// Validate one endpoint URL and its optional notifier metadata.
    ///
    /// # Errors
    ///
    /// Returns the [`EndpointRefusal`] when no delivery binding this broker was
    /// built with serves the endpoint's scheme, or when the binding that does
    /// cannot honour the `notifierInfo` the endpoint carries.
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal>;
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint_admission::{SubscriptionEndpointValidator, validate_endpoint},
        endpoint_refusal::EndpointRefusal,
        error::SubscriptionError,
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use url::Url;

    struct RejectingEndpointValidator;

    impl SubscriptionEndpointValidator for RejectingEndpointValidator {
        fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
            Err(EndpointRefusal::UnsupportedScheme)
        }
    }

    #[test]
    fn endpoint_validation_rejects_an_invalid_url_before_calling_the_binding() {
        let payload = serde_json::json!({"notification": {"endpoint": {"uri": "://invalid"}}});
        assert!(matches!(
            validate_endpoint(&payload, &RejectingEndpointValidator),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointUri { .. }))
        ));
    }

    #[test]
    fn endpoint_validation_propagates_the_selected_binding_refusal() {
        let payload = serde_json::json!({"notification": {"endpoint": {"uri": "http://example.com/notify"}}});
        let error = validate_endpoint(&payload, &RejectingEndpointValidator);
        assert!(matches!(
            error,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRejected(
                EndpointRefusal::UnsupportedScheme
            )))
        ));
    }
}
