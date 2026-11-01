//! The ways a loaded configuration breaks a rule.
//!
//! A variant is one shape of rule; the keys it is about and the values they
//! hold are its fields, so every rule of one shape reads alike and a test
//! compares the refusal itself rather than its wording.

use crate::{
    human_duration::HumanDuration,
    schema::{http::forwarded::ForwardedTrust, tls::ClientAuthenticationMode},
    validate::{
        bound::{Bound, Quantity, Relation},
        config_key::ConfigKey,
    },
};
use derive_more::Display;
use std::time::Duration;
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
    /// A key another setting makes necessary is left out.
    #[display("{key} must be set while {required_by}")]
    Unset {
        /// The key left out.
        key: ConfigKey,
        /// The setting that needs it.
        required_by: Requirement,
    },
    /// A client CA bundle is configured while no client certificate is asked
    /// for, so nothing would read it.
    #[display("tls.client_authentication.ca is set but tls.client_authentication.mode is \"disabled\"; set the mode or remove the bundle")]
    ClientCaWithoutClientAuthentication,
    /// A separate management plane binds the data-plane port. A wildcard host
    /// overlaps every host, so the planes are compared on port alone.
    #[display("management.port ({port}) must differ from server.port while management.mode is \"separate-port\"")]
    ManagementPortCollision {
        /// The port both planes bind.
        port: u16,
    },
    /// Metrics are enabled while no management plane exists to serve them.
    #[display(
        "observability.metrics.prometheus.mode is \"enabled\" but management.mode is \"disabled\"; set management.mode to \"separate-port\" or \"shared-port\", or disable metrics"
    )]
    MetricsWithoutManagementPlane,
    /// The CORS policy allows credentials beside an `any` wildcard, which the
    /// Fetch standard forbids and the CORS layer cannot build.
    #[display("http.cors: credentials = \"allowed\" cannot be combined with an `any` wildcard on origins, methods, headers or exposed headers")]
    CorsCredentialsWithWildcard,
    /// Proxies are listed while the trust believes no list, so nothing would
    /// read them.
    #[display("http.forwarded.proxies is set but http.forwarded.trust is \"{trust}\"; set the trust to \"proxies\" or remove the list")]
    ProxiesWithoutProxyTrust {
        /// The trust that reads no list.
        trust: ForwardedTrust,
    },
    /// Two preloaded contexts name one URL, so which file serves it would be
    /// decided by their order.
    #[display("jsonld.preloaded_contexts names {url} more than once")]
    DuplicatePreloadedContext {
        /// The URL named more than once.
        url: Url,
    },
}

/// The setting that makes a left-out key necessary.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Requirement {
    /// The data-plane listener terminates TLS.
    #[display("server.transport is \"tls\"")]
    ServerTls,
    /// The HTTP/3 listener is enabled, and QUIC always terminates TLS.
    #[display("server.http3.mode is \"enabled\"")]
    Http3,
    /// The separate management listener terminates TLS.
    #[display("management.transport is \"tls\" and management.mode is \"separate-port\"")]
    ManagementTls,
    /// Client certificates are asked for.
    #[display("tls.client_authentication.mode is \"{_0}\"")]
    ClientAuthentication(ClientAuthenticationMode),
    /// History retention is enabled.
    #[display("temporal.storage.retention.mode is \"enabled\"")]
    Retention,
    /// Forwarded fields are believed only from listed proxies.
    #[display("http.forwarded.trust is \"proxies\"")]
    ProxyTrust,
}

#[cfg(test)]
mod tests {
    use crate::{
        schema::tls::ClientAuthenticationMode,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            violation::{ConfigurationViolation, Requirement},
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
            required_by: Requirement::ClientAuthentication(ClientAuthenticationMode::Required),
        };
        assert_eq!(
            violation.to_string(),
            "tls.client_authentication.ca must be set while tls.client_authentication.mode is \"required\""
        );
    }
}
