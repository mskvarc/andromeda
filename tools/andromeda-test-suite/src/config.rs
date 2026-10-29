//! The one configuration of the suite: where the brokers of the stack listen, how they reach the test process back,
//! and the limits the stack configures them with.
//!
//! Every value comes from the environment under the prefix `ANDROMEDA_TEST_SUITE__`, with `__` separating the nested
//! keys, the format Andromeda reads its own configuration in (`ANDROMEDA_TEST_SUITE__BROKERS__FRONT`). Nothing else in
//! the suite reads the environment. A value a test needs that the stack does not name fails the test with the key it
//! should have been given.

use crate::config_error::{ConfigError, ConfigKey};
use figment2::{Figment, providers::Env};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    net::{IpAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};
use strum::IntoStaticStr;
use url::{Host, Url};

/// The prefix every key of the suite's configuration carries in the environment.
pub(crate) const PREFIX: &str = "ANDROMEDA_TEST_SUITE__";

/// A broker of a stack, by the role the stack gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum BrokerRole {
    /// The broker every test talks to unless it names another: broker A of the multi-broker stacks.
    Front,
    /// Broker B of the multi-broker stacks.
    B,
    /// Broker C of the multi-broker stacks.
    C,
    /// Broker D of the multi-broker stacks.
    D,
    /// Broker E of the multi-broker stacks, running temporal Linked Entity retrieval.
    E,
    /// Broker F of the multi-broker stacks, reading `q` target languages as extended ranges.
    F,
    /// Broker G of the multi-broker stacks, reading `q` target languages as extended ranges.
    G,
    /// The andromeda stack broker in conformance mode.
    Conformance,
    /// The andromeda stack broker that leaves Core writes out of the Temporal Evolution.
    NotRecorded,
    /// The andromeda stack broker that records Core writes and runs no expiry sweep.
    Recorded,
    /// The andromeda stack broker that terminates TLS itself.
    Tls,
    /// The management listener of the TLS broker.
    TlsManagement,
    /// The andromeda stack broker holding at most three Snapshots per Tenant.
    Snapshot,
    /// The andromeda stack broker with multi-tenancy disabled.
    SingleTenant,
    /// The andromeda stack broker delivering notifications in durable mode.
    Durable,
    /// The andromeda stack broker believing the `Forwarded`, `X-Forwarded-Proto` and `X-Forwarded-Host` fields.
    Edge,
    /// The edge broker's authority under `https`.
    EdgeHttps,
    /// The andromeda stack broker configured with a path-prefixed apiRoot.
    Prefixed,
    /// The andromeda stack broker reading `q` target languages as RFC 4647 extended ranges.
    LanguageRange,
    /// The andromeda stack broker admitting every RFC 5646 tag as a `q` target language.
    LanguageTags,
}

/// A broker as another broker of the stack reaches it: what a registration `endpoint` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PeerRole {
    /// Broker A of the multi-broker stacks.
    A,
    /// Broker B of the multi-broker stacks.
    B,
    /// Broker C of the multi-broker stacks.
    C,
    /// Broker D of the multi-broker stacks.
    D,
    /// Broker E of the multi-broker stacks.
    E,
    /// Broker F of the multi-broker stacks.
    F,
    /// Broker G of the multi-broker stacks.
    G,
    /// The andromeda stack broker in conformance mode.
    Conformance,
}

/// A limit the stack configures its brokers with, which a test reaches or crosses on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum Budget {
    /// The bytes of attribute values one request may carry.
    RequestValue,
    /// The bytes of attribute values one entity may hold.
    EntityValue,
    /// The positions one geometry may hold on every broker but the recorded one.
    Position,
    /// The positions one geometry may hold on the recorded broker.
    LargePosition,
    /// The bytes of the largest request body a broker reads.
    MaxRequestBody,
}

/// A host name or address, written as a URL host is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct HostName(Host<String>);

// `url::Host` deserializes from its enum form; the environment holds the host as text.
impl TryFrom<String> for HostName {
    type Error = url::ParseError;

    fn try_from(text: String) -> Result<HostName, url::ParseError> {
        Host::parse(&text).map(HostName)
    }
}

impl HostName {
    /// The host.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        &self.0
    }
}

/// The MQTT brokers notifications are published to.
#[derive(Debug, Clone, Deserialize)]
pub struct MqttSettings {
    /// The anonymous Mosquitto broker, as the brokers and the test process reach it.
    host: HostName,
    /// The Mosquitto broker refusing anonymous clients on the ports endpoints name.
    auth_host: HostName,
    /// The listener port other than 1883 both Mosquitto brokers serve.
    non_default_port: u16,
    /// The listener without a mount point an observer subscribes on, so that every topic carries the port it was
    /// published through.
    observer_port: u16,
}

impl MqttSettings {
    /// The anonymous Mosquitto broker.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        self.host.host()
    }

    /// The Mosquitto broker that requires credentials.
    #[must_use]
    pub const fn auth_host(&self) -> &Host<String> {
        self.auth_host.host()
    }

    /// The listener port other than 1883.
    #[must_use]
    pub const fn non_default_port(&self) -> u16 {
        self.non_default_port
    }

    /// The listener an observer subscribes on.
    #[must_use]
    pub const fn observer_port(&self) -> u16 {
        self.observer_port
    }
}

/// The configuration of the stack the suite runs against.
#[derive(Debug, Clone, Deserialize)]
pub struct SuiteConfig {
    /// Every broker the stack runs, by role, as the test process reaches it.
    #[serde(default)]
    brokers: BTreeMap<BrokerRole, Url>,
    /// Every broker as another broker reaches it.
    #[serde(default)]
    peers: BTreeMap<PeerRole, Url>,
    /// The host the brokers reach the test process by; when absent, the address the test process reaches the front
    /// broker from.
    callback_host: Option<HostName>,
    /// The MQTT brokers, on a stack that runs them.
    mqtt: Option<MqttSettings>,
    /// The limits the brokers are configured with.
    #[serde(default)]
    budgets: BTreeMap<Budget, u64>,
    /// The NGSI-LD core `@context` the brokers answer with.
    core_context: Url,
    /// An origin the brokers resolve where nothing listens.
    unreachable_origin: Url,
    /// The apiRoot the prefixed broker advertises.
    api_root_prefixed: Option<Url>,
    /// The directory of the stack's files (its TLS certificates and keys) as the test process reads it.
    stack_dir: Option<PathBuf>,
    /// The host the test process connects to in place of the host a URL names: a TLS broker whose certificate
    /// names `localhost` is addressed as `localhost`, so the certificate matches, and reached at its own container.
    #[serde(default)]
    connect_to: BTreeMap<HostName, HostName>,
    /// The longest a positive wait waits, in seconds; `None` is
    /// [`crate::eventually::DEFAULT_DELIVERY_DEADLINE`].
    delivery_deadline_seconds: Option<u64>,
}

/// The configuration, read once per test process.
static CONFIG: OnceLock<Result<SuiteConfig, ConfigError>> = OnceLock::new();

/// The callback host found from the route to the front broker, once per test process.
static DISCOVERED_CALLBACK_HOST: OnceLock<Result<Host<String>, ConfigError>> = OnceLock::new();

impl SuiteConfig {
    /// The configuration of the stack, read from the environment on first use.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] when a value is missing or malformed, naming it.
    pub fn get() -> Result<&'static SuiteConfig, &'static ConfigError> {
        CONFIG
            .get_or_init(|| SuiteConfig::read(&Figment::new().merge(Env::prefixed(PREFIX).split("__"))))
            .as_ref()
    }

    /// Reads the configuration `figment` holds.
    fn read(figment: &Figment) -> Result<SuiteConfig, ConfigError> {
        figment.extract().map_err(|error| ConfigError::Invalid(Box::new(error)))
    }

    /// The longest a positive wait waits, when the stack sets it.
    #[must_use]
    pub fn delivery_deadline(&self) -> Option<Duration> {
        self.delivery_deadline_seconds.map(Duration::from_secs)
    }

    /// The broker of `role`, as the test process reaches it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack runs no such broker.
    pub fn broker(&self, role: BrokerRole) -> Result<&Url, ConfigError> {
        self.brokers.get(&role).ok_or_else(|| missing("BROKERS", role.into()))
    }

    /// Every broker the stack runs, by role, as the test process reaches it.
    pub fn brokers(&self) -> impl Iterator<Item = (BrokerRole, &Url)> {
        self.brokers.iter().map(|(role, url)| (*role, url))
    }

    /// Every broker as another broker reaches it, by role.
    pub fn peers(&self) -> impl Iterator<Item = (PeerRole, &Url)> {
        self.peers.iter().map(|(role, url)| (*role, url))
    }

    /// The broker of `role`, as another broker reaches it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such peer.
    pub fn peer(&self, role: PeerRole) -> Result<&Url, ConfigError> {
        self.peers.get(&role).ok_or_else(|| missing("PEERS", role.into()))
    }

    /// The limit `budget` the brokers are configured with.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such limit.
    pub fn budget(&self, budget: Budget) -> Result<u64, ConfigError> {
        self.budgets.get(&budget).copied().ok_or_else(|| missing("BUDGETS", budget.into()))
    }

    /// The MQTT brokers.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn mqtt(&self) -> Result<&MqttSettings, ConfigError> {
        self.mqtt.as_ref().ok_or(ConfigError::Missing(ConfigKey::Mqtt))
    }

    /// The NGSI-LD core `@context` the brokers answer with.
    #[must_use]
    pub const fn core_context(&self) -> &Url {
        &self.core_context
    }

    /// An origin the brokers resolve where nothing listens.
    #[must_use]
    pub const fn unreachable_origin(&self) -> &Url {
        &self.unreachable_origin
    }

    /// The apiRoot the prefixed broker advertises.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack without the prefixed broker.
    pub fn api_root_prefixed(&self) -> Result<&Url, ConfigError> {
        self.api_root_prefixed.as_ref().ok_or(ConfigError::Missing(ConfigKey::ApiRootPrefixed))
    }

    /// Each host a URL may name and the host the test process connects to in its place.
    pub fn connect_to(&self) -> impl Iterator<Item = (&HostName, &HostName)> {
        self.connect_to.iter()
    }

    /// The directory of the stack's files.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that names none.
    pub fn stack_dir(&self) -> Result<&Path, ConfigError> {
        self.stack_dir.as_deref().ok_or(ConfigError::Missing(ConfigKey::StackDir))
    }

    /// The host the brokers reach the test process by.
    ///
    /// A configured value wins. Otherwise it is the local address of the route to the front broker, which is the
    /// address of this container on the stack network: each runner container has its own, so two runs against one
    /// stack never receive each other's callbacks.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when no front broker is configured or no route to it exists.
    pub fn callback_host(&self) -> Result<Host<String>, ConfigError> {
        match &self.callback_host {
            Some(host) => Ok(host.host().clone()),
            None => DISCOVERED_CALLBACK_HOST.get_or_init(|| self.discover_callback_host()).clone(),
        }
    }

    /// The local address of the route to the front broker.
    fn discover_callback_host(&self) -> Result<Host<String>, ConfigError> {
        let front = self.broker(BrokerRole::Front)?;
        // clone: the error names the front broker the configuration keeps
        let host = front.host_str().ok_or_else(|| ConfigError::FrontWithoutHost(front.clone()))?;
        let port = front.port_or_known_default().unwrap_or(80);
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| ConfigError::RouteSocket(Arc::new(error)))?;
        socket.connect((host, port)).map_err(|error| ConfigError::NoRoute {
            // clone: the error names the front broker the configuration keeps
            front: front.clone(),
            source: Arc::new(error),
        })?;
        let local = socket.local_addr().map_err(|error| ConfigError::RouteSocket(Arc::new(error)))?;
        Ok(match local.ip() {
            IpAddr::V4(address) => Host::Ipv4(address),
            IpAddr::V6(address) => Host::Ipv6(address),
        })
    }
}

/// The error for the key `name` of the section `section`.
const fn missing(section: &'static str, name: &'static str) -> ConfigError {
    ConfigError::Missing(ConfigKey::Entry { section, name })
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{BrokerRole, Budget, PeerRole, SuiteConfig},
        config_error::ConfigError,
    };
    use figment2::{Figment, providers::Serialized, value::Value};
    use googletest::prelude::*;
    use url::{Host, Url};

    /// A figment holding `pairs` as the environment provider reads them: the key after the prefix lowercased and
    /// split at `__`, the value parsed as the provider parses it.
    fn figment(pairs: &[(&str, &str)]) -> Figment {
        pairs.iter().fold(Figment::new(), |figment, (key, text)| {
            let Ok(value) = text.parse::<Value>();
            figment.merge(Serialized::global(&key.to_ascii_lowercase().replace("__", "."), value))
        })
    }

    /// The pairs every stack sets.
    const REQUIRED: [(&str, &str); 3] = [
        ("BROKERS__FRONT", "http://broker:1056"),
        ("CORE_CONTEXT", "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"),
        ("UNREACHABLE_ORIGIN", "http://runner:1"),
    ];

    #[gtest]
    fn a_configured_broker_peer_and_budget_are_read_by_role() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.extend([
            ("BROKERS__NOT_RECORDED", "http://broker-not-recorded:1073"),
            ("PEERS__A", "http://broker-a:1061"),
            ("BUDGETS__MAX_REQUEST_BODY", "67108864"),
        ]);
        let config = SuiteConfig::read(&figment(&pairs))?;
        expect_that!(config.broker(BrokerRole::Front).map(Url::as_str), ok(eq(&"http://broker:1056/")));
        expect_that!(
            config.broker(BrokerRole::NotRecorded).map(Url::as_str),
            ok(eq(&"http://broker-not-recorded:1073/"))
        );
        expect_that!(config.peer(PeerRole::A).map(Url::as_str), ok(eq(&"http://broker-a:1061/")));
        expect_that!(config.budget(Budget::MaxRequestBody), ok(eq(&67_108_864)));
        Ok(())
    }

    #[gtest]
    fn an_absent_broker_names_the_key_the_stack_should_set() -> Result<()> {
        let config = SuiteConfig::read(&figment(&REQUIRED))?;
        let error = config.broker(BrokerRole::Conformance).err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__BROKERS__CONFORMANCE"))
    }

    #[gtest]
    fn a_malformed_url_is_refused_with_its_key() -> Result<()> {
        let result = SuiteConfig::read(&figment(&[("BROKERS__FRONT", "not a url"), REQUIRED[1], REQUIRED[2]]));
        let Err(ConfigError::Invalid(error)) = result else {
            return fail!("a malformed broker URL was accepted");
        };
        verify_that!(error.to_string(), contains_substring("front"))
    }

    #[gtest]
    fn a_configured_callback_host_wins_over_discovery() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.push(("CALLBACK_HOST", "runner"));
        let config = SuiteConfig::read(&figment(&pairs))?;
        verify_that!(config.callback_host()?, eq(&Host::Domain("runner".to_owned())))
    }

    #[gtest]
    fn the_mqtt_settings_are_read_as_one_section() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.extend([
            ("MQTT__HOST", "mosquitto"),
            ("MQTT__AUTH_HOST", "mosquitto-auth"),
            ("MQTT__NON_DEFAULT_PORT", "8085"),
            ("MQTT__OBSERVER_PORT", "1880"),
        ]);
        let config = SuiteConfig::read(&figment(&pairs))?;
        let mqtt = config.mqtt()?;
        expect_that!(mqtt.host(), eq(&Host::Domain("mosquitto".to_owned())));
        expect_that!(mqtt.non_default_port(), eq(8085));
        expect_that!(mqtt.observer_port(), eq(1880));
        Ok(())
    }
}
