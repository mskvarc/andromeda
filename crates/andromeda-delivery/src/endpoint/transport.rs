//! Which transport a subscription's endpoint URI names.

use crate::{
    endpoint::{
        error::EndpointParseError,
        mqtt::{MqttEndpointConfig, MqttQoS, MqttVersion},
        topic::MqttTopic,
    },
    host_port::HostPort,
};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use secrecy::SecretString;
use std::str::FromStr;
use url::Url;

/// Default MQTT port for a plaintext connection.
const MQTT_DEFAULT_PORT: u16 = 1883;

/// Default MQTT port for a TLS connection.
const MQTTS_DEFAULT_PORT: u16 = 8883;

/// Notification delivery transport.
///
/// Chosen entirely by the endpoint URI's scheme (clause 5.2.15). `sse` and
/// `ws` are Andromeda extensions: they name a connection the client opens and
/// the broker pushes down, so the URI's authority and path carry no meaning
/// and are not read.
#[derive(Debug, Clone)]
pub enum EndpointTransport {
    /// `http` or `https` — one POST per notification.
    Http,
    /// `mqtt` or `mqtts` — a publish on a pooled broker connection.
    Mqtt(MqttEndpointConfig),
    /// `sse` — pushed to whichever clients are streaming this subscription.
    Sse,
    /// `ws` — pushed to whichever clients hold a WebSocket for this subscription.
    WebSocket,
}

impl MqttVersion {
    /// The version a subscription's `notifierInfo` names.
    ///
    /// The accepted spellings are the ones clause 7.2 defines, not the ones
    /// [`MqttVersion`]'s `Display` produces: `Display` is for a log line and
    /// this is a wire value, and letting the two share a table would quietly
    /// widen what the broker accepts.
    fn from_notifier_info(value: &str) -> Result<MqttVersion, EndpointParseError> {
        match value {
            "mqtt5.0" => Ok(MqttVersion::V5),
            "mqtt3.1.1" => Ok(MqttVersion::V311),
            other => Err(EndpointParseError::UnsupportedMqttVersion { value: other.to_string() }),
        }
    }
}

impl MqttQoS {
    /// The quality of service a subscription's `notifierInfo` names.
    fn from_notifier_info(value: &str) -> Result<MqttQoS, EndpointParseError> {
        match value {
            "0" => Ok(MqttQoS::AtMost),
            "1" => Ok(MqttQoS::AtLeast),
            "2" => Ok(MqttQoS::Exactly),
            other => Err(EndpointParseError::UnsupportedMqttQos { value: other.to_string() }),
        }
    }
}

/// Parse an endpoint URI, with the subscription's `notifierInfo`, into the
/// transport it names.
pub fn parse_endpoint_transport(uri: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<EndpointTransport, EndpointParseError> {
    match uri.scheme() {
        "http" | "https" => Ok(EndpointTransport::Http),
        "sse" => Ok(EndpointTransport::Sse),
        "ws" => Ok(EndpointTransport::WebSocket),
        "mqtt" | "mqtts" => Ok(EndpointTransport::Mqtt(parse_mqtt_endpoint(uri, notifier_info)?)),
        other => Err(EndpointParseError::UnsupportedScheme { scheme: other.to_string() }),
    }
}

/// Read the broker, topic and credentials out of an `mqtt` or `mqtts` URI.
fn parse_mqtt_endpoint(parsed: &Url, notifier_info: Option<&[KeyValuePair]>) -> Result<MqttEndpointConfig, EndpointParseError> {
    let use_tls = parsed.scheme() == "mqtts";
    let host = parsed.host().ok_or(EndpointParseError::MissingHost)?.to_owned();
    let default_port = if use_tls { MQTTS_DEFAULT_PORT } else { MQTT_DEFAULT_PORT };
    let port = parsed.port().unwrap_or(default_port);
    let authority = HostPort::new(host, port);

    // The empty path is its own error, kept distinct from a path that is present
    // but not a valid publish topic.
    let topic_path = parsed.path().trim_start_matches('/');
    if topic_path.is_empty() {
        return Err(EndpointParseError::MissingTopic);
    }
    let topic = MqttTopic::from_str(topic_path)?;

    let username = if parsed.username().is_empty() {
        None
    } else {
        Some(parsed.username().to_string())
    };
    let password = parsed.password().map(SecretString::from);

    let mut version = MqttVersion::V5;
    let mut qos = MqttQoS::AtMost;
    for info in notifier_info.unwrap_or_default() {
        match info.key.as_str() {
            "MQTT-Version" => version = MqttVersion::from_notifier_info(&info.value)?,
            "MQTT-QoS" => qos = MqttQoS::from_notifier_info(&info.value)?,
            _other => {}
        }
    }
    let (version, qos) = (version, qos);

    // Rebuilt from the validated parts rather than cloned from `parsed`, so the
    // stored endpoint URI never carries the userinfo the original may have
    // embedded. The authority and topic came from a URI that already parsed, so
    // the canonical form parses too.
    let scheme = if use_tls { "mqtts" } else { "mqtt" };
    let endpoint_uri = Url::parse(&format!("{scheme}://{authority}/{topic}"))?;

    Ok(MqttEndpointConfig {
        authority,
        topic,
        endpoint_uri,
        use_tls,
        version,
        qos,
        username,
        password,
    })
}

#[cfg(test)]
mod tests {
    use crate::endpoint::{
        error::EndpointParseError,
        mqtt::{MqttEndpointConfig, MqttQoS, MqttVersion},
        transport::{EndpointTransport, parse_endpoint_transport as parse_typed_endpoint_transport},
    };
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use secrecy::ExposeSecret;
    use url::Url;

    fn parse_endpoint_transport(uri: &str, info: Option<&[KeyValuePair]>) -> Result<EndpointTransport, EndpointParseError> {
        let uri = Url::parse(uri)?;
        parse_typed_endpoint_transport(&uri, info)
    }

    fn notifier_info(pairs: &[(&str, &str)]) -> Vec<KeyValuePair> {
        pairs
            .iter()
            .map(|(key, value)| KeyValuePair {
                key: (*key).into(),
                value: (*value).into(),
            })
            .collect()
    }

    fn mqtt_config(uri: &str, info: Option<&[KeyValuePair]>) -> MqttEndpointConfig {
        match parse_endpoint_transport(uri, info).expect("the URI parses") {
            EndpointTransport::Mqtt(config) => config,
            EndpointTransport::Http | EndpointTransport::Sse | EndpointTransport::WebSocket => panic!("expected an MQTT transport for {uri}"),
        }
    }

    #[test]
    fn an_http_endpoint_and_an_https_endpoint_both_resolve_to_the_http_transport() {
        assert!(matches!(
            parse_endpoint_transport("http://example.com/notify", None).expect("parses"),
            EndpointTransport::Http
        ));
        assert!(matches!(
            parse_endpoint_transport("https://example.com/notify", None).expect("parses"),
            EndpointTransport::Http
        ));
    }

    #[test]
    fn an_mqtt_uri_yields_its_broker_topic_and_defaults() {
        let config = mqtt_config("mqtt://broker.example.com:1883/test/topic", None);
        pretty_assertions::assert_eq!(config.authority.to_string(), "broker.example.com:1883");
        pretty_assertions::assert_eq!(config.topic.as_str(), "test/topic");
        assert!(!config.use_tls);
        pretty_assertions::assert_eq!(config.version, MqttVersion::V5);
        pretty_assertions::assert_eq!(config.qos, MqttQoS::AtMost);
    }

    #[test]
    fn an_mqtt_uri_without_a_port_takes_1883_and_an_mqtts_uri_takes_8883() {
        pretty_assertions::assert_eq!(mqtt_config("mqtt://broker.example.com/topic", None).authority.port(), 1883);

        let secure = mqtt_config("mqtts://broker.example.com/secure/topic", None);
        pretty_assertions::assert_eq!(secure.authority.port(), 8883);
        assert!(secure.use_tls);
        pretty_assertions::assert_eq!(secure.topic.as_str(), "secure/topic");
    }

    #[test]
    fn notifier_info_overrides_the_version_and_the_quality_of_service() {
        let info = notifier_info(&[("MQTT-Version", "mqtt3.1.1"), ("MQTT-QoS", "2")]);
        let config = mqtt_config("mqtt://broker:1883/topic", Some(&info));
        pretty_assertions::assert_eq!(config.version, MqttVersion::V311);
        pretty_assertions::assert_eq!(config.qos, MqttQoS::Exactly);

        let at_least = notifier_info(&[("MQTT-QoS", "1")]);
        pretty_assertions::assert_eq!(mqtt_config("mqtt://broker:1883/topic", Some(&at_least)).qos, MqttQoS::AtLeast);
    }

    #[test]
    fn a_notifier_info_key_this_broker_does_not_read_is_ignored_rather_than_rejected() {
        let info = notifier_info(&[("X-Vendor-Setting", "whatever"), ("MQTT-QoS", "1")]);
        pretty_assertions::assert_eq!(mqtt_config("mqtt://broker:1883/topic", Some(&info)).qos, MqttQoS::AtLeast);
    }

    #[test]
    fn an_mqtt_uri_with_no_topic_is_rejected_because_there_is_nothing_to_publish_to() {
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883", None),
            Err(EndpointParseError::MissingTopic | EndpointParseError::InvalidUri(_))
        ));
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883/", None),
            Err(EndpointParseError::MissingTopic)
        ));
    }

    #[test]
    fn an_mqtt_uri_whose_topic_carries_a_publish_wildcard_is_rejected() {
        // `+` is legal in a URL path but not in an MQTT publish topic, so it
        // reaches the topic validator and is refused there. (`#` cannot be
        // tested through a URI: the URL parser reads it as the fragment
        // delimiter, so the topic newtype's own tests cover it.)
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883/sensors/+/data", None),
            Err(EndpointParseError::InvalidTopic(_))
        ));
    }

    #[test]
    fn the_streaming_schemes_resolve_regardless_of_authority_path_or_case() {
        for uri in ["sse://internal", "sse://internal/anything", "SSE://internal"] {
            assert!(matches!(parse_endpoint_transport(uri, None).expect("parses"), EndpointTransport::Sse), "{uri}");
        }
        for uri in ["ws://internal", "ws://internal/anything", "WS://internal"] {
            assert!(
                matches!(parse_endpoint_transport(uri, None).expect("parses"), EndpointTransport::WebSocket),
                "{uri}"
            );
        }
    }

    #[test]
    fn a_scheme_this_broker_cannot_deliver_over_is_rejected() {
        assert!(matches!(
            parse_endpoint_transport("ftp://example.com/path", None),
            Err(EndpointParseError::UnsupportedScheme { .. })
        ));
    }

    #[test]
    fn a_notifier_info_value_outside_what_clause_7_2_defines_is_rejected() {
        let version = notifier_info(&[("MQTT-Version", "mqtt4.0")]);
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883/topic", Some(&version)),
            Err(EndpointParseError::UnsupportedMqttVersion { .. })
        ));

        let qos = notifier_info(&[("MQTT-QoS", "3")]);
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883/topic", Some(&qos)),
            Err(EndpointParseError::UnsupportedMqttQos { .. })
        ));
    }

    #[test]
    fn the_display_spelling_of_a_version_is_not_accepted_as_a_notifier_info_value() {
        let info = notifier_info(&[("MQTT-Version", "v5")]);
        assert!(matches!(
            parse_endpoint_transport("mqtt://broker:1883/topic", Some(&info)),
            Err(EndpointParseError::UnsupportedMqttVersion { .. })
        ));
    }

    #[test]
    fn credentials_in_the_uri_userinfo_reach_the_broker_configuration() {
        let both = mqtt_config("mqtt://myuser:mypass@broker:1883/topic", None);
        pretty_assertions::assert_eq!(both.username.as_deref(), Some("myuser"));
        pretty_assertions::assert_eq!(both.password.as_ref().map(|secret| secret.expose_secret()), Some("mypass"));
        pretty_assertions::assert_eq!(both.authority.to_string(), "broker:1883");

        let username_only = mqtt_config("mqtt://myuser@broker/topic", None);
        pretty_assertions::assert_eq!(username_only.username.as_deref(), Some("myuser"));
        assert!(username_only.password.is_none());

        let neither = mqtt_config("mqtt://broker/topic", None);
        assert!(neither.username.is_none());
        assert!(neither.password.is_none());
    }

    #[test]
    fn a_broker_password_does_not_appear_in_the_debug_rendering() {
        let config = mqtt_config("mqtt://myuser:hunter2@broker/topic", None);
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(rendered.contains("myuser"), "{rendered}");
    }
}
