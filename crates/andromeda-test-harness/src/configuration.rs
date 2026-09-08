//! Environment-derived integration-test configuration and redacted diagnostics.

use andromeda_test_client::{
    broker_endpoint::{BROKER_ENDPOINT_ENV, BrokerEndpoint, default_broker_endpoint},
    transport_error::EndpointError,
};
use std::{env, fmt};
use thiserror::Error;
use url::{Host, Url};

/// Environment variable containing the PostgreSQL control/tenant endpoint.
pub const DATABASE_ENDPOINT_ENV: &str = "ANDROMEDA_TEST_DATABASE_URL";

/// Default PostgreSQL endpoint used by local integration tests.
pub const DEFAULT_DATABASE_ENDPOINT: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";

/// Environment variable containing the broker-visible callback host.
pub const CALLBACK_HOST_ENV: &str = "ANDROMEDA_TEST_CALLBACK_HOST";

/// Default callback host for a broker running in Docker Desktop.
pub const DEFAULT_CALLBACK_HOST: &str = "host.docker.internal";

/// Runtime-rendered broker API base for direct reqwest suites awaiting endpoint-client migration.
pub const RUNTIME_BROKER_BASE: RuntimeBrokerBase = RuntimeBrokerBase;

/// Display adapter that reads the broker base from the environment at request construction time.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeBrokerBase;

impl fmt::Display for RuntimeBrokerBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let configured = env::var(BROKER_ENDPOINT_ENV).unwrap_or_else(|_| default_broker_endpoint().to_owned());
        formatter.write_str(configured.trim_end_matches('/'))
    }
}

/// Validated PostgreSQL control and default-tenant endpoint.
#[derive(Clone)]
pub struct DatabaseEndpoint {
    url: Url,
}

impl DatabaseEndpoint {
    /// Parse a PostgreSQL URL.
    pub fn parse(value: impl AsRef<str>) -> Result<DatabaseEndpoint, ConfigurationError> {
        let url = Url::parse(value.as_ref()).map_err(ConfigurationError::InvalidDatabaseEndpoint)?;
        match url.scheme() {
            "postgres" | "postgresql" => Ok(DatabaseEndpoint { url }),
            other => Err(ConfigurationError::InvalidDatabaseScheme(other.to_owned())),
        }
    }

    /// Borrow the complete URL for `SQLx` without exposing it through diagnostics.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    fn redacted(&self) -> Url {
        let mut redacted = self.url.clone(); // clone: diagnostics redact an owned copy without changing the connection endpoint.
        if redacted.password().is_some() {
            let _result = redacted.set_password(Some("REDACTED"));
        }
        redacted
    }
}

/// Broker-visible callback host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackEndpoint {
    host: Host<String>,
}

impl CallbackEndpoint {
    /// Parse a DNS name or IP address used by the broker for callbacks.
    pub fn parse(value: impl AsRef<str>) -> Result<CallbackEndpoint, ConfigurationError> {
        let host = Host::parse(value.as_ref()).map_err(ConfigurationError::InvalidCallbackHost)?;
        Ok(CallbackEndpoint { host })
    }

    /// Borrow the validated host.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        &self.host
    }
}

/// Complete runtime configuration read once by a test process or suite.
#[derive(Clone)]
pub struct TestRunConfiguration {
    broker: BrokerEndpoint,
    database: DatabaseEndpoint,
    callback: CallbackEndpoint,
}

impl TestRunConfiguration {
    /// Read and validate every integration-test environment setting.
    pub fn from_environment() -> Result<TestRunConfiguration, ConfigurationError> {
        let broker = BrokerEndpoint::from_environment().map_err(ConfigurationError::InvalidBrokerEndpoint)?;
        let database_value = env::var(DATABASE_ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_DATABASE_ENDPOINT.to_owned());
        let callback_value = env::var(CALLBACK_HOST_ENV).unwrap_or_else(|_| DEFAULT_CALLBACK_HOST.to_owned());
        Ok(TestRunConfiguration {
            broker,
            database: DatabaseEndpoint::parse(database_value)?,
            callback: CallbackEndpoint::parse(callback_value)?,
        })
    }

    /// Borrow the broker endpoint.
    #[must_use]
    pub const fn broker(&self) -> &BrokerEndpoint {
        &self.broker
    }

    /// Borrow the database endpoint.
    #[must_use]
    pub const fn database(&self) -> &DatabaseEndpoint {
        &self.database
    }

    /// Borrow the callback endpoint.
    #[must_use]
    pub const fn callback(&self) -> &CallbackEndpoint {
        &self.callback
    }

    /// Render a failure-oriented summary with database credentials redacted.
    #[must_use]
    pub const fn diagnostic_summary(&self) -> ConfigurationSummary<'_> {
        ConfigurationSummary { configuration: self }
    }
}

/// Redacted display wrapper for test-run diagnostics.
pub struct ConfigurationSummary<'a> {
    configuration: &'a TestRunConfiguration,
}

impl fmt::Display for ConfigurationSummary<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "broker={}, database={}, callback={}",
            self.configuration.broker.url(),
            self.configuration.database.redacted(),
            self.configuration.callback.host()
        )
    }
}

/// Invalid integration-test configuration.
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// The broker URL is invalid.
    #[error("invalid {BROKER_ENDPOINT_ENV}: {0}")]
    InvalidBrokerEndpoint(#[source] EndpointError),
    /// The database URL is invalid.
    #[error("invalid {DATABASE_ENDPOINT_ENV}: {0}")]
    InvalidDatabaseEndpoint(#[source] url::ParseError),
    /// The database URL does not use a PostgreSQL scheme.
    #[error("database endpoint must use postgres or postgresql, got {0}")]
    InvalidDatabaseScheme(String),
    /// The callback host is invalid.
    #[error("invalid {CALLBACK_HOST_ENV}: {0}")]
    InvalidCallbackHost(#[source] url::ParseError),
}

#[cfg(test)]
mod tests {
    use crate::configuration::{CallbackEndpoint, DatabaseEndpoint, TestRunConfiguration};
    use andromeda_test_client::broker_endpoint::BrokerEndpoint;

    #[test]
    fn diagnostic_summary_never_contains_the_database_password() {
        let configuration = TestRunConfiguration {
            broker: BrokerEndpoint::parse("http://127.0.0.1:1056/ngsi-ld/v1/").unwrap_or_else(|error| unreachable!("static broker URL is valid: {error}")),
            database: DatabaseEndpoint::parse("postgres://alice:secret@127.0.0.1/db")
                .unwrap_or_else(|error| unreachable!("static database URL is valid: {error}")),
            callback: CallbackEndpoint::parse("127.0.0.1").unwrap_or_else(|error| unreachable!("static callback host is valid: {error}")),
        };

        let summary = configuration.diagnostic_summary().to_string();

        assert!(!summary.contains("secret"));
        assert!(summary.contains("REDACTED"));
    }
}
