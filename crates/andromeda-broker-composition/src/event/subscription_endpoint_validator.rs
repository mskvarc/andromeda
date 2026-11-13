//! Delivery-backed validation of subscription endpoint configuration.

use andromeda_delivery::endpoint::{error::EndpointParseError, transport::admit_endpoint_transport};
use andromeda_ngsi_ld_model::{http_header_field::HttpHeaderFieldError, key_value_pair::KeyValuePair, notification_header_field::NotificationHeaderFieldError};
use andromeda_subscription::{endpoint_admission::SubscriptionEndpointValidator, endpoint_refusal::EndpointRefusal};
use url::Url;

/// Validates endpoint configuration against the broker's selected delivery bindings.
pub(crate) struct DeliverySubscriptionEndpointValidator;

impl SubscriptionEndpointValidator for DeliverySubscriptionEndpointValidator {
    fn validate(&self, endpoint: &Url, notifier_info: Option<&[KeyValuePair]>, receiver_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
        admit_endpoint_transport(endpoint, notifier_info, receiver_info)
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
        EndpointParseError::ReceiverInfo(NotificationHeaderFieldError::Field(HttpHeaderFieldError::Name(_))) => EndpointRefusal::ReceiverInfoHeaderName,
        EndpointParseError::ReceiverInfo(NotificationHeaderFieldError::Field(HttpHeaderFieldError::Value(_))) => EndpointRefusal::ReceiverInfoHeaderValue,
        EndpointParseError::ReceiverInfo(NotificationHeaderFieldError::Reserved(header)) => EndpointRefusal::ReceiverInfoReservedHeader(*header),
    }
}

#[cfg(test)]
mod tests {
    use crate::event::subscription_endpoint_validator::DeliverySubscriptionEndpointValidator;
    use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, notification_header_field::ReservedHeader};
    use andromeda_subscription::{endpoint_admission::SubscriptionEndpointValidator, endpoint_refusal::EndpointRefusal};
    use url::Url;

    fn validate(endpoint: &str, key: &str) -> Result<(), EndpointRefusal> {
        let endpoint = Url::parse(endpoint).expect("a well-formed endpoint");
        let receiver_info = [KeyValuePair {
            key: key.into(),
            value: "x".into(),
        }];
        DeliverySubscriptionEndpointValidator.validate(&endpoint, None, Some(&receiver_info))
    }

    #[test]
    fn a_receiver_info_key_naming_a_reserved_header_is_refused_naming_that_header() {
        pretty_assertions::assert_eq!(
            validate("http://example.com/notify", "Transfer-Encoding"),
            Err(EndpointRefusal::ReceiverInfoReservedHeader(ReservedHeader::TransferEncoding))
        );
    }

    #[test]
    fn a_receiver_info_key_that_is_no_header_name_keeps_its_own_refusal() {
        pretty_assertions::assert_eq!(validate("http://example.com/notify", "Bad Name"), Err(EndpointRefusal::ReceiverInfoHeaderName));
    }

    #[test]
    fn a_custom_receiver_info_key_is_admitted() {
        pretty_assertions::assert_eq!(validate("https://example.com/notify", "Authorization"), Ok(()));
    }
}
