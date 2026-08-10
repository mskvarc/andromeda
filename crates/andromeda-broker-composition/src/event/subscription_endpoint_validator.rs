//! Delivery-backed validation of subscription endpoint configuration.

use andromeda_delivery::endpoint::transport::parse_endpoint_transport;
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use andromeda_subscription::{error::SubscriptionError, lifecycle::SubscriptionEndpointValidator};
use url::Url;

/// Validates endpoint configuration against the broker's selected delivery bindings.
pub struct DeliverySubscriptionEndpointValidator;

impl SubscriptionEndpointValidator for DeliverySubscriptionEndpointValidator {
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<(), SubscriptionError> {
        parse_endpoint_transport(endpoint, notifier_info)
            .map(|_transport| ())
            .map_err(|error| SubscriptionError::validation(error.to_string()))
    }
}
