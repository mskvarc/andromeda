//! The ways a loaded configuration breaks a rule.
//!
//! A variant is one shape of rule; the keys it is about and the values they
//! hold are its fields, so every rule of one shape reads alike and a test
//! compares the refusal itself rather than its wording.

use crate::{
    human_duration::HumanDuration,
    validate::{
        bound::{Bound, Quantity, Relation},
        choice_value::ChoiceValue,
        config_key::ConfigKey,
    },
};
use derive_more::Display;
use std::{
    fmt::{self, Formatter},
    time::Duration,
};
use url::Url;

/// One rule a loaded configuration breaks.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub enum ConfigurationViolation {
    /// A value that must be positive is zero.
    #[display("{key} must be greater than zero")]
    Zero {
        /// The key holding zero.
        key: ConfigKey,
    },
    /// A value does not stand in the required relation to its bound.
    #[display("{key} ({value}) must be {relation} {bound}")]
    OutOfRange {
        /// The key holding the value.
        key: ConfigKey,
        /// The value it holds.
        value: Quantity,
        /// How the value must relate to the bound.
        relation: Relation,
        /// What the value is compared against.
        bound: Bound,
    },
    /// A day-grained duration is not a positive whole number of days.
    #[display("{key} ({}) must be a positive whole number of days", HumanDuration::new(*value))]
    NotWholeDays {
        /// The key holding the duration.
        key: ConfigKey,
        /// The duration it holds.
        value: Duration,
    },
    /// A byte size does not fit the platform's address width, so the limit
    /// it sets could never be reached.
    #[display("{key} ({bytes} bytes) does not fit the platform address width")]
    ExceedsAddressWidth {
        /// The key holding the size.
        key: ConfigKey,
        /// The size it holds, in bytes.
        bytes: u64,
    },
    /// A key the settings in force make necessary is left out.
    #[display("{key} must be set{required_by}")]
    Unset {
        /// The key left out.
        key: ConfigKey,
        /// The settings that need it.
        required_by: Requirement,
    },
    /// A key is set while the settings in force make nothing read it.
    #[display("{key} must be left out{required_by}")]
    Unread {
        /// The key set.
        key: ConfigKey,
        /// The settings under which nothing reads it.
        required_by: Requirement,
    },
    /// A key holds a keyword the settings in force refuse.
    #[display("{key} must not be \"{choice}\"{required_by}")]
    ChoiceRefused {
        /// The key holding the keyword.
        key: ConfigKey,
        /// The keyword it holds.
        choice: ChoiceValue,
        /// The settings that refuse it.
        required_by: Requirement,
    },
    /// Two keys that must differ hold the same value. A wildcard host
    /// overlaps every host, so two listeners are compared on port alone.
    #[display("{key} ({value}) must differ from {other}{required_by}")]
    Collision {
        /// The key read.
        key: ConfigKey,
        /// The key it must differ from.
        other: ConfigKey,
        /// The value both hold.
        value: Quantity,
        /// The settings under which they must differ.
        required_by: Requirement,
    },
    /// Two entries of a list of tables hold the same URL, so which of them
    /// applies would be decided by their order.
    #[display("{key} is {url} in more than one entry")]
    Duplicate {
        /// The key the entries share.
        key: ConfigKey,
        /// The URL held more than once.
        url: Url,
    },
}

/// A keyword a key holds, as a refusal names it.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[display("{key} is \"{choice}\"")]
pub struct HeldChoice {
    /// The key.
    pub key: ConfigKey,
    /// The keyword it holds.
    pub choice: ChoiceValue,
}

impl HeldChoice {
    /// `key` holding `choice`.
    #[must_use]
    pub fn new(key: ConfigKey, choice: impl Into<ChoiceValue>) -> HeldChoice {
        HeldChoice { key, choice: choice.into() }
    }
}

/// The settings under which a broken rule applies, as the configuration holds
/// them; empty for a rule that always applies.
///
/// Displayed as the clause a refusal ends with, ` while` and every setting
/// joined by `and`, and as nothing when empty.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Requirement(Vec<HeldChoice>);

impl Requirement {
    /// The settings `held`, in the order the rule names them.
    #[must_use]
    pub fn new(held: impl IntoIterator<Item = HeldChoice>) -> Requirement {
        Requirement(held.into_iter().collect())
    }
}

// Manual: a derived `Display` formats fixed fields, and the clause joins a
// list of any length, or is nothing.
impl fmt::Display for Requirement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.iter().enumerate().try_for_each(|(index, held)| {
            let joiner = if index == 0 { " while" } else { " and" };
            write!(formatter, "{joiner} {held}")
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        schema::{listener_transport::ListenerTransport, management::ManagementMode, tls::ClientAuthenticationMode},
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            violation::{ConfigurationViolation, HeldChoice, Requirement},
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn an_out_of_range_value_names_both_keys_and_their_values() {
        let violation = ConfigurationViolation::OutOfRange {
            key: ConfigKey::DatabaseAcquireTimeout,
            value: Quantity::Duration(Duration::from_secs(30)),
            relation: Relation::Below,
            bound: Bound::setting(ConfigKey::RequestLimitsTimeout, Duration::from_secs(30)),
        };
        assert_eq!(
            violation.to_string(),
            "database.acquire_timeout (30s) must be below request_limits.timeout (30s)"
        );
    }

    #[test]
    fn an_unset_key_names_the_setting_that_needs_it() {
        let violation = ConfigurationViolation::Unset {
            key: ConfigKey::TlsClientAuthenticationCa,
            required_by: Requirement::new([HeldChoice::new(ConfigKey::TlsClientAuthenticationMode, ClientAuthenticationMode::Required)]),
        };
        assert_eq!(
            violation.to_string(),
            "tls.client_authentication.ca must be set while tls.client_authentication.mode is \"required\""
        );
    }

    #[test]
    fn a_refusal_names_every_setting_it_applies_under_joined_by_and() {
        let violation = ConfigurationViolation::Unset {
            key: ConfigKey::TlsCertificate,
            required_by: Requirement::new([
                HeldChoice::new(ConfigKey::ManagementTransport, ListenerTransport::Tls),
                HeldChoice::new(ConfigKey::ManagementMode, ManagementMode::SeparatePort),
            ]),
        };
        assert_eq!(
            violation.to_string(),
            "tls.certificate must be set while management.transport is \"tls\" and management.mode is \"separate-port\""
        );
    }

    #[test]
    fn a_rule_that_always_applies_names_no_setting() {
        let violation = ConfigurationViolation::Unset {
            key: ConfigKey::TlsCertificate,
            required_by: Requirement::default(),
        };
        assert_eq!(violation.to_string(), "tls.certificate must be set");
    }
}
