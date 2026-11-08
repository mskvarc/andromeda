//! Admitting a subscription's notification endpoint (ETSI GS CIM 009 clause
//! 5.2.15).
//!
//! Which endpoints a broker can reach is a property of the delivery bindings
//! it was built with, not of the subscription resource, so the rule is a trait
//! the composition implements and the lifecycle merely calls. Create and
//! update ask it the same question, and an update fragment that touches only
//! the endpoint has to be admitted exactly as a create carrying the same
//! endpoint would be.

use crate::{endpoint_refusal::EndpointRefusal, error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, subscription::Endpoint};
use url::Url;

/// Reject a subscription whose endpoint names a transport this broker cannot
/// deliver over, a `notifierInfo` it cannot honour, or a `receiverInfo` the
/// endpoint's binding cannot send.
///
/// Takes the endpoint a request carries rather than a stored `Subscription`
/// because an update carries a fragment, and the same check has to hold for a
/// create and for a patch that touches only the endpoint. That the endpoint
/// `uri` is a URL and `notifierInfo` an array of key-value pairs is the
/// request model's reading, so no endpoint reaches here that is not.
///
/// # Errors
///
/// Returns [`SubscriptionError::Validation`] when the binding refuses the
/// endpoint.
pub(crate) fn validate_endpoint(endpoint: Option<&Endpoint>, validator: &dyn SubscriptionEndpointValidator) -> Result<(), SubscriptionError> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    validator
        .validate(endpoint.uri.url(), endpoint.notifier_info.as_deref(), endpoint.receiver_info.as_deref())
        .map_err(|refusal| SubscriptionError::Validation(SubscriptionValidationReason::EndpointRejected(refusal)))
}

/// Validates whether endpoint configuration can be served by the selected delivery bindings.
pub trait SubscriptionEndpointValidator: Send + Sync + 'static {
    /// Validate one endpoint URL with its optional notifier and receiver metadata.
    ///
    /// # Errors
    ///
    /// Returns the [`EndpointRefusal`] when no delivery binding this broker was
    /// built with serves the endpoint's scheme, or when the binding that does
    /// cannot honour the `notifierInfo` or `receiverInfo` the endpoint carries.
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>, receiver_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal>;
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint_admission::{SubscriptionEndpointValidator, validate_endpoint},
        endpoint_refusal::EndpointRefusal,
        error::SubscriptionError,
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, subscription::Endpoint};
    use serde_json::json;
    use url::Url;

    struct RejectingEndpointValidator;

    impl SubscriptionEndpointValidator for RejectingEndpointValidator {
        fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>, receiver_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
            match receiver_info {
                Some(_pairs) => Err(EndpointRefusal::ReceiverInfoHeaderName),
                None => Err(EndpointRefusal::UnsupportedScheme),
            }
        }
    }

    #[test]
    fn an_invalid_endpoint_url_is_refused_when_the_endpoint_is_read_before_any_binding_is_asked() {
        assert!(serde_json::from_value::<Endpoint>(json!({"uri": "://invalid"})).is_err());
    }

    #[test]
    fn endpoint_validation_propagates_the_selected_binding_refusal() {
        let endpoint: Endpoint = serde_json::from_value(json!({"uri": "http://example.com/notify"})).expect("a readable endpoint");
        let error = validate_endpoint(Some(&endpoint), &RejectingEndpointValidator);
        assert!(matches!(
            error,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRejected(
                EndpointRefusal::UnsupportedScheme
            )))
        ));
    }

    #[test]
    fn endpoint_validation_hands_the_binding_the_receiver_info_the_endpoint_carries() {
        let endpoint: Endpoint = serde_json::from_value(json!({"uri": "http://example.com/notify", "receiverInfo": [{"key": "Bad Name", "value": "x"}]}))
            .expect("a readable endpoint");
        let error = validate_endpoint(Some(&endpoint), &RejectingEndpointValidator);
        assert!(matches!(
            error,
            Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRejected(
                EndpointRefusal::ReceiverInfoHeaderName
            )))
        ));
    }

    #[test]
    fn a_request_naming_no_endpoint_has_nothing_to_validate() {
        assert!(validate_endpoint(None, &RejectingEndpointValidator).is_ok());
    }
}
