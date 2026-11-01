//! Why a value of the test configuration a test needs is not available, and the key it should have been given.

use crate::config::PREFIX;
use std::{fmt, io, sync::Arc};
use url::Url;

/// Why a value the test needs is not available.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    /// The environment does not describe a configuration.
    #[error("the test configuration in the {PREFIX}* environment is invalid: {0}")]
    Invalid(#[source] Box<figment2::Error>),
    /// The stack names no value for the key.
    #[error("the stack sets no {0}")]
    Missing(ConfigKey),
    /// The front broker's URL names no host to find the route to.
    #[error("the address the brokers reach this process by could not be found: {0} names no host")]
    FrontWithoutHost(Url),
    /// No socket could be opened to find the route to the front broker.
    #[error("the address the brokers reach this process by could not be found")]
    RouteSocket(#[source] Arc<io::Error>),
    /// No route leads to the front broker.
    #[error("the address the brokers reach this process by could not be found: no route to {front}")]
    NoRoute {
        /// The front broker.
        front: Url,
        /// Why the route could not be opened.
        #[source]
        source: Arc<io::Error>,
    },
}

impl ConfigError {
    /// The error for the absent entry `name` of the section `section`.
    pub(crate) const fn missing(section: &'static str, name: &'static str) -> ConfigError {
        ConfigError::Missing(ConfigKey::Entry { section, name })
    }
}

/// A key of the suite configuration, named as its environment variable spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    /// The entry `name` of the section `section`.
    Entry {
        /// The section, such as `BROKERS`.
        section: &'static str,
        /// The entry of the section.
        name: &'static str,
    },
    /// Every key of the MQTT section.
    Mqtt,
    /// Every key of the pooling section.
    Pooling,
    /// The apiRoot of the prefixed broker.
    ApiRootPrefixed,
    /// The directory of the stack's files.
    StackDir,
}

// Hand-written: an entry is spelled from its section and name, behind the shared prefix.
impl fmt::Display for ConfigKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigKey::Entry { section, name } => write!(formatter, "{PREFIX}{section}__{name}"),
            ConfigKey::Mqtt => write!(formatter, "{PREFIX}MQTT__*"),
            ConfigKey::Pooling => write!(formatter, "{PREFIX}POOLING__*"),
            ConfigKey::ApiRootPrefixed => write!(formatter, "{PREFIX}API_ROOT_PREFIXED"),
            ConfigKey::StackDir => write!(formatter, "{PREFIX}STACK_DIR"),
        }
    }
}
