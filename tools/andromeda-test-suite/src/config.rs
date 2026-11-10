//! The one configuration of the tests: where the brokers of the stack listen, how they reach the test process back, and
//! the limits the stack configures them with.
//!
//! Every value comes from the environment under the prefix `ANDROMEDA_TEST_SUITE__`, with `__` separating the nested
//! keys, the format Andromeda reads its own configuration in (`ANDROMEDA_TEST_SUITE__BROKERS__FRONT`). Nothing else in
//! the tests reads the environment. A value a test needs that the stack does not name fails the test with the key it
//! should have been given.

use crate::{
    budget::Budget,
    config_error::{ConfigError, ConfigKey},
    host_name::HostName,
    mqtt_settings::MqttSettings,
    pooling_settings::PoolingSettings,
    server_settings::ServerSettings,
};
use figment2::{Figment, providers::Env};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
use strum::IntoStaticStr;
use url::{Host, Url};

/// The prefix every key of the test configuration carries in the environment.
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
    /// Broker E of the multi-broker stacks; on the multi stack it runs temporal Linked Entity retrieval and reads `q`
    /// target languages as extended ranges.
    E,
    /// Broker F of the multi stack, reading `q` target languages as extended ranges and gathering at most 5 distinct
    /// entities and 15 source rows for one forwarded query.
    F,
    /// The andromeda stack broker in conformance mode.
    Conformance,
    /// The andromeda stack broker that leaves Core writes out of the Temporal Evolution, keeps every other setting at
    /// its default (the extension flags and the geometry position budget included), advertises no URL of its own and
    /// believes forwarded fields only from a proxy range the runner is outside of.
    NotRecorded,
    /// The andromeda stack broker that terminates TLS itself.
    Tls,
    /// The TLS broker's API port addressed in plaintext.
    TlsPlaintext,
    /// The management listener of the TLS broker.
    TlsManagement,
    /// The TLS broker's management port addressed in plaintext.
    TlsManagementPlaintext,
    /// The andromeda stack broker that terminates TLS itself with a TLS 1.3 floor and serves only clients presenting a
    /// certificate of the stack's client authority.
    Mutual,
    /// The andromeda stack broker that terminates TLS itself and whose identity a test rotates.
    Rotating,
    /// The TLS terminator in front of the edge broker.
    Terminator,
    /// The terminator's plaintext listener, which redirects to its TLS listener.
    TerminatorPlaintext,
    /// The andromeda stack broker with multi-tenancy disabled, delivering notifications durably with a 4 KiB request
    /// body limit, a 5 KiB HTTP notification body limit and four attempts.
    SingleTenant,
    /// The andromeda stack broker delivering notifications in durable mode and holding at most three Snapshots per
    /// Tenant.
    Durable,
    /// The andromeda stack broker believing the `Forwarded`, `X-Forwarded-Proto` and `X-Forwarded-Host` fields,
    /// reading `q` target languages as RFC 4647 extended ranges and asking context sources for every instance of an
    /// aggregated temporal query.
    Edge,
    /// The edge broker's authority under `https`.
    EdgeHttps,
    /// The andromeda stack broker configured with a path-prefixed apiRoot and admitting every RFC 5646 tag as a `q`
    /// target language.
    Prefixed,
    /// The andromeda stack broker serving its Prometheus metrics, which only the metrics tests talk to, so a series
    /// it exposes moves only with what those tests do.
    Observed,
    /// The andromeda stack broker keeping four tenants loaded, the default tenant among them, waking a tenant no
    /// earlier than five seconds after it was unloaded and keeping a woken tenant loaded for one second.
    Waker,
    /// The andromeda stack broker keeping four tenants loaded, the default tenant among them, waking a tenant no
    /// earlier than a second after it was unloaded and keeping a woken tenant loaded for five seconds, only the test of
    /// a wake load it cannot serve uses.
    Backlog,
    /// The andromeda stack broker keeping 41 tenants loaded, the default tenant among them, waking a tenant no earlier
    /// than five seconds after it was unloaded and keeping a woken tenant loaded for five seconds, only the test of a
    /// wake load it can serve uses.
    Roomy,
    /// The andromeda stack broker with the backlog broker's places and wake timings, only the test of what its waker
    /// does while no place is free uses.
    Overloaded,
    /// The andromeda stack broker serving its Prometheus metrics, only the test of notifications refused for their
    /// size uses, so the oversized-notification counter moves only with what that test sends.
    Oversized,
    /// The andromeda stack broker giving a request head two seconds and an idle connection six, only the tests of
    /// connections that carry no request use, since the short idle bound would close the pooled connections of any
    /// other test under it.
    Timed,
    /// The pgbouncer stack broker keeping at most 32 tenants resident and delivering notifications durably, whose
    /// evicted tenants the dormant tests watch.
    Dormant,
    /// The pgbouncer stack broker keeping at most three tenants resident and delivering notifications live through an
    /// evaluation queue of two changes and one lane, whose evicted tenants must still have been evaluated in full.
    DormantLive,
    /// The pgbouncer stack broker whose pooler the resilience tests pause, reconnect, saturate and kill, one tenant
    /// database at a time.
    Resilience,
    /// The pgbouncer stack broker delivering notifications durably and running maintenance, behind a pooler with two
    /// server connections per database and a small prepared-statement cache.
    PooledDurable,
    /// The pgbouncer stack broker reaching its PostgreSQL server directly, with no pooler.
    Direct,
    /// The pgbouncer stack broker only the provisioning-burst scale test uses.
    Provisioning,
    /// The pgbouncer stack broker only the scale test of a working set larger than its cache uses.
    Thrash,
    /// The pgbouncer stack broker only the scale test of a Zipf working set uses.
    Zipf,
    /// The pgbouncer stack broker keeping every tenant resident, only the idle-cache scale test uses.
    Wide,
    /// The pgbouncer stack broker keeping every tenant resident, only the scale test of a new tenant among hundreds of
    /// resident ones uses.
    Crowded,
    /// The pgbouncer stack broker only the scale test of client concurrency against backends uses.
    Concurrency,
    /// The pooled pgbouncer stack broker of the burst-footprint scale test.
    BurstPooled,
    /// The un-pooled pgbouncer stack broker of the burst-footprint scale test.
    BurstDirect,
    /// The pgbouncer stack broker behind a pooler with two server connections per database, only the test of first
    /// writes into new tenants while the wake reconciliation pass probes them uses.
    FirstWrite,
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
    /// Broker F of the multi stack.
    F,
}

/// The configuration of the stack the tests run against.
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
    /// The anonymous Mosquitto broker announcing a maximum packet size below the brokers' `max_mqtt_packet`, on the
    /// andromeda stack.
    capped_mqtt_host: Option<HostName>,
    /// The service that signals a broker's process, on the pgbouncer stack.
    signaller: Option<Url>,
    /// The poolers behind the brokers and the size of the scale tests, on the pgbouncer stack.
    pooling: Option<PoolingSettings>,
    /// The PostgreSQL server each broker a test reaches the database of owns.
    #[serde(default)]
    servers: ServerSettings,
    /// The Prometheus exposition each broker that serves one serves on its management listener, by role.
    #[serde(default)]
    metrics: BTreeMap<BrokerRole, Url>,
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
    /// The directory a broker reads its TLS certificate and key from (`tls.crt`, `tls.key`), by role, as the test
    /// process writes it: only a broker whose identity tests rewrite is named.
    #[serde(default)]
    identity_dirs: BTreeMap<BrokerRole, PathBuf>,
    /// The longest a positive wait waits, in seconds; `None` is
    /// [`crate::eventually::DEFAULT_DELIVERY_DEADLINE`].
    delivery_deadline_seconds: Option<u64>,
}

/// The configuration, read once per test process.
static CONFIG: OnceLock<Result<SuiteConfig, ConfigError>> = OnceLock::new();

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
        self.brokers.get(&role).ok_or_else(|| ConfigError::missing("BROKERS", role.into()))
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
        self.peers.get(&role).ok_or_else(|| ConfigError::missing("PEERS", role.into()))
    }

    /// The limit `budget` the brokers are configured with.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such limit.
    pub fn budget(&self, budget: Budget) -> Result<u64, ConfigError> {
        self.budgets.get(&budget).copied().ok_or_else(|| ConfigError::missing("BUDGETS", budget.into()))
    }

    /// The MQTT brokers.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn mqtt(&self) -> Result<&MqttSettings, ConfigError> {
        self.mqtt.as_ref().ok_or(ConfigError::Missing(ConfigKey::Mqtt))
    }

    /// The anonymous Mosquitto broker announcing a maximum packet size below the brokers' `max_mqtt_packet`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn capped_mqtt_host(&self) -> Result<&Host<String>, ConfigError> {
        self.capped_mqtt_host
            .as_ref()
            .map(HostName::host)
            .ok_or(ConfigError::Missing(ConfigKey::CappedMqttHost))
    }

    /// The service that signals a broker's process.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that runs none.
    pub fn signaller(&self) -> Result<&Url, ConfigError> {
        self.signaller.as_ref().ok_or(ConfigError::Missing(ConfigKey::Signaller))
    }

    /// The poolers behind the brokers and the size of the scale tests.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that names none.
    pub fn pooling(&self) -> Result<&PoolingSettings, ConfigError> {
        self.pooling.as_ref().ok_or(ConfigError::Missing(ConfigKey::Pooling))
    }

    /// The PostgreSQL server each broker a test reaches the database of owns.
    #[must_use]
    pub const fn servers(&self) -> &ServerSettings {
        &self.servers
    }

    /// The Prometheus exposition of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such exposition.
    pub fn metrics(&self, role: BrokerRole) -> Result<&Url, ConfigError> {
        self.metrics.get(&role).ok_or_else(|| ConfigError::missing("METRICS", role.into()))
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

    /// The directory the broker of `role` reads its TLS identity from.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack names no such directory.
    pub fn identity_dir(&self, role: BrokerRole) -> Result<&Path, ConfigError> {
        self.identity_dirs
            .get(&role)
            .map(PathBuf::as_path)
            .ok_or_else(|| ConfigError::missing("IDENTITY_DIRS", role.into()))
    }

    /// The directory of the stack's files.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] on a stack that names none.
    pub fn stack_dir(&self) -> Result<&Path, ConfigError> {
        self.stack_dir.as_deref().ok_or(ConfigError::Missing(ConfigKey::StackDir))
    }

    /// The host the brokers reach the test process by, when the stack sets it.
    #[must_use]
    pub fn configured_callback_host(&self) -> Option<&Host<String>> {
        self.callback_host.as_ref().map(HostName::host)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        budget::Budget,
        config::{BrokerRole, PeerRole, SuiteConfig},
        config_error::{ConfigError, ConfigKey},
        database_name::DatabaseName,
        front_route::callback_host,
    };
    use figment2::{Figment, providers::Serialized, value::Value};
    use googletest::prelude::*;
    use std::{path::Path, time::Duration};
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
    fn an_identity_directory_is_read_by_role_and_an_absent_one_names_its_key() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.push(("IDENTITY_DIRS__ROTATING", "/identities/rotating"));
        let config = SuiteConfig::read(&figment(&pairs))?;
        expect_that!(config.identity_dir(BrokerRole::Rotating), ok(eq(&Path::new("/identities/rotating"))));
        let Err(ConfigError::Missing(key)) = config.identity_dir(BrokerRole::Mutual) else {
            return fail!("an identity directory the stack does not set was found");
        };
        verify_that!(
            key,
            eq(ConfigKey::Entry {
                section: "IDENTITY_DIRS",
                name: "MUTUAL"
            })
        )
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
        verify_that!(callback_host(&config)?, eq(&Host::Domain("runner".to_owned())))
    }

    #[gtest]
    fn the_pooling_settings_are_read_as_one_section_by_role() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.extend([
            ("POOLING__POOLERS__DORMANT", "postgres://andromeda:andromeda@pgbouncer-dormant:6432/pgbouncer"),
            ("POOLING__SCALE_TENANTS", "100"),
            ("POOLING__LOAD_SECONDS", "60"),
        ]);
        let config = SuiteConfig::read(&figment(&pairs))?;
        let pooling = config.pooling()?;
        expect_that!(pooling.pooler(BrokerRole::Dormant).map(Url::path), ok(eq(&"/pgbouncer")));
        expect_that!(pooling.scale_tenants(), eq(100));
        expect_that!(pooling.load_duration(), eq(Duration::from_secs(60)));
        let error = pooling.pooler(BrokerRole::Direct).err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__POOLING__POOLERS__DIRECT"))
    }

    #[gtest]
    fn a_server_is_read_by_role_and_names_a_tenant_database_and_a_missing_one_names_its_key() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.push(("SERVERS__DURABLE", "postgres://andromeda:andromeda@postgres-durable:5432/andromeda"));
        let config = SuiteConfig::read(&figment(&pairs))?;
        expect_that!(
            config.servers().server(BrokerRole::Durable).map(Url::as_str),
            ok(eq(&"postgres://andromeda:andromeda@postgres-durable:5432/andromeda"))
        );
        expect_that!(
            config
                .servers()
                .database(BrokerRole::Durable, &DatabaseName::from("andromeda_tenant_acme_0badc0de".to_owned()))
                .map(String::from),
            ok(eq("postgres://andromeda:andromeda@postgres-durable:5432/andromeda_tenant_acme_0badc0de"))
        );
        let error = config.servers().server(BrokerRole::Dormant).err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__SERVERS__DORMANT"))
    }

    #[gtest]
    fn the_metrics_expositions_are_read_by_role_and_a_missing_one_names_its_key() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.push(("METRICS__OBSERVED", "http://broker-observed:1086/metrics"));
        let config = SuiteConfig::read(&figment(&pairs))?;
        expect_that!(
            config.metrics(BrokerRole::Observed).map(Url::as_str),
            ok(eq(&"http://broker-observed:1086/metrics"))
        );
        let error = config.metrics(BrokerRole::Dormant).err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__METRICS__DORMANT"))
    }

    #[gtest]
    fn a_stack_without_a_pooling_section_names_its_keys() -> Result<()> {
        let config = SuiteConfig::read(&figment(&REQUIRED))?;
        let error = config.pooling().err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__POOLING__*"))
    }

    #[gtest]
    fn the_mqtt_settings_are_read_as_one_section() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.extend([
            ("MQTT__HOST", "mosquitto"),
            ("MQTT__AUTH_HOST", "mosquitto-auth"),
            ("MQTT__LARGE_HOST", "mosquitto-large"),
            ("MQTT__NON_DEFAULT_PORT", "8085"),
            ("MQTT__OBSERVER_PORT", "1880"),
        ]);
        let config = SuiteConfig::read(&figment(&pairs))?;
        let mqtt = config.mqtt()?;
        expect_that!(mqtt.host(), eq(&Host::Domain("mosquitto".to_owned())));
        expect_that!(mqtt.large_host()?, eq(&Host::Domain("mosquitto-large".to_owned())));
        expect_that!(mqtt.non_default_port(), eq(8085));
        expect_that!(mqtt.observer_port(), eq(1880));
        Ok(())
    }

    #[gtest]
    fn the_capped_mqtt_host_is_read_on_its_own_and_a_missing_one_names_its_key() -> Result<()> {
        let mut pairs = REQUIRED.to_vec();
        pairs.push(("CAPPED_MQTT_HOST", "mosquitto-capped"));
        let config = SuiteConfig::read(&figment(&pairs))?;
        expect_that!(config.capped_mqtt_host()?, eq(&Host::Domain("mosquitto-capped".to_owned())));
        let error = SuiteConfig::read(&figment(&REQUIRED))?.capped_mqtt_host().err().or_fail()?;
        verify_that!(error.to_string(), eq("the stack sets no ANDROMEDA_TEST_SUITE__CAPPED_MQTT_HOST"))
    }
}
