//! Delivery-backed validation of subscription endpoint configuration.

use andromeda_delivery::endpoint::{error::EndpointParseError, transport::parse_endpoint_transport};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use andromeda_subscription::{endpoint_admission::SubscriptionEndpointValidator, endpoint_refusal::EndpointRefusal};
use url::Url;

/// Validates endpoint configuration against the broker's selected delivery bindings.
pub struct DeliverySubscriptionEndpointValidator;

impl SubscriptionEndpointValidator for DeliverySubscriptionEndpointValidator {
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
        parse_endpoint_transport(endpoint, notifier_info)
            .map(|_transport| ())
            .map_err(|error| endpoint_refusal(&error))
    }
}

/// The subscription-facing refusal a delivery binding's parse failure is.
///
/// An endpoint that is not a URI never reaches the bindings: the subscription
/// policy parses it before asking, so that variant is the scheme the bindings
/// could not serve.
const fn endpoint_refusal(error: &EndpointParseError) -> EndpointRefusal {
    match error {
        EndpointParseError::InvalidUri(_) | EndpointParseError::UnsupportedScheme => EndpointRefusal::UnsupportedScheme,
        EndpointParseError::MissingHost => EndpointRefusal::MqttHostMissing,
        EndpointParseError::MissingTopic => EndpointRefusal::MqttTopicMissing,
        EndpointParseError::InvalidTopic(_) => EndpointRefusal::MqttTopicInvalid,
        EndpointParseError::UnsupportedMqttVersion => EndpointRefusal::UnsupportedMqttVersion,
        EndpointParseError::UnsupportedMqttQos => EndpointRefusal::UnsupportedMqttQos,
    }
}
