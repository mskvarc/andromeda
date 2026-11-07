//! A validated MQTT publish topic.

use std::str::FromStr;

/// The largest a topic may be, in bytes: the MQTT topic length is a two-byte
/// prefixed field, so it cannot exceed 65535 bytes (MQTT v5 clause 1.5.4).
const MAX_TOPIC_BYTES: usize = 65_535;

/// An MQTT topic a notification may be *published* to.
///
/// A publish topic is not a subscription filter: MQTT reserves `+` (single
/// level) and `#` (multi level) for subscriptions and forbids them in the topic
/// of a PUBLISH (MQTT v5 clause 4.7.1), so this newtype rejects them at
/// construction. It also rejects the empty topic (clause 4.7.3) and the null
/// character (clause 1.5.4), so a value of this type is always a topic a broker
/// will accept rather than reject on receipt.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct MqttTopic(String);

impl MqttTopic {
    /// The topic as a string slice, for the publish API that takes one.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check a candidate against the publish-topic rules, naming the first
    /// violation found.
    fn validate(topic: &str) -> Result<(), MqttTopicError> {
        if topic.is_empty() {
            return Err(MqttTopicError::Empty);
        }
        if topic.len() > MAX_TOPIC_BYTES {
            return Err(MqttTopicError::TooLong { length: topic.len() });
        }
        if let Some(wildcard) = topic.chars().find(|&character| character == '+' || character == '#') {
            return Err(MqttTopicError::Wildcard { wildcard });
        }
        if topic.contains('\0') {
            return Err(MqttTopicError::NullCharacter);
        }
        Ok(())
    }
}

/// Why a string is not a valid MQTT publish topic.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MqttTopicError {
    /// The topic was empty; a topic must have at least one character.
    #[error("an MQTT publish topic must not be empty")]
    Empty,
    /// The topic carried a wildcard, which only a subscription filter may.
    #[error("an MQTT publish topic must not contain the wildcard character '{wildcard}'")]
    Wildcard {
        /// The wildcard character that was found, `+` or `#`.
        wildcard: char,
    },
    /// The topic contained the null character, which MQTT forbids in any topic.
    #[error("an MQTT publish topic must not contain the null character")]
    NullCharacter,
    /// The topic exceeded the MQTT length limit.
    #[error("an MQTT publish topic must not exceed {MAX_TOPIC_BYTES} bytes, but this one is {length} bytes")]
    TooLong {
        /// The length of the rejected topic, in bytes.
        length: usize,
    },
}

impl FromStr for MqttTopic {
    type Err = MqttTopicError;

    fn from_str(topic: &str) -> Result<MqttTopic, MqttTopicError> {
        MqttTopic::validate(topic)?;
        Ok(MqttTopic(topic.to_owned()))
    }
}

impl TryFrom<String> for MqttTopic {
    type Error = MqttTopicError;

    fn try_from(topic: String) -> Result<MqttTopic, MqttTopicError> {
        MqttTopic::validate(&topic)?;
        Ok(MqttTopic(topic))
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoint::topic::{MqttTopic, MqttTopicError};
    use std::str::FromStr;

    #[test]
    fn a_plain_topic_round_trips_through_its_string_form() {
        let topic = MqttTopic::from_str("sensors/room-1/temperature").expect("a valid topic");
        pretty_assertions::assert_eq!(topic.as_str(), "sensors/room-1/temperature");
        pretty_assertions::assert_eq!(topic.to_string(), "sensors/room-1/temperature");
    }

    #[test]
    fn the_owned_and_borrowed_constructors_accept_the_same_topics() {
        let from_owned = MqttTopic::try_from("a/b/c".to_owned()).expect("a valid topic");
        let from_borrowed = MqttTopic::from_str("a/b/c").expect("a valid topic");
        pretty_assertions::assert_eq!(from_owned, from_borrowed);
    }

    #[test]
    fn a_single_level_wildcard_is_rejected_because_it_is_a_subscription_construct() {
        pretty_assertions::assert_eq!(MqttTopic::from_str("sensors/+/temperature"), Err(MqttTopicError::Wildcard { wildcard: '+' }));
    }

    #[test]
    fn a_multi_level_wildcard_is_rejected_because_it_is_a_subscription_construct() {
        pretty_assertions::assert_eq!(MqttTopic::from_str("sensors/#"), Err(MqttTopicError::Wildcard { wildcard: '#' }));
    }

    #[test]
    fn the_empty_topic_is_rejected_because_there_is_nothing_to_publish_to() {
        pretty_assertions::assert_eq!(MqttTopic::from_str(""), Err(MqttTopicError::Empty));
        pretty_assertions::assert_eq!(MqttTopic::try_from(String::new()), Err(MqttTopicError::Empty));
    }

    #[test]
    fn a_topic_carrying_a_null_character_is_rejected() {
        pretty_assertions::assert_eq!(MqttTopic::from_str("a/\0/b"), Err(MqttTopicError::NullCharacter));
    }

    #[test]
    fn a_topic_over_the_length_limit_is_rejected() {
        let oversized = "a".repeat(65_536);
        pretty_assertions::assert_eq!(MqttTopic::from_str(&oversized), Err(MqttTopicError::TooLong { length: 65_536 }));
    }
}
